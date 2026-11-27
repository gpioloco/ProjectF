use crate::{
    apply,
    design::*,
    palette,
    store::{Library, Store},
    theme::{Effects, Palette, Photo, Theme, Wallpaper, hex},
    wallpaper,
};
use anyhow::{Context as _, Result};
use eframe::egui::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Library,
    Editor,
}
#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Presets,
    Custom,
}
struct Preview {
    source: Wallpaper,
    effects: Effects,
    texture: TextureHandle,
}
struct PhotoSample {
    thumbnail: image::RgbImage,
    palette: Option<Palette>,
}

impl PhotoSample {
    fn load(source: &Wallpaper) -> Result<Self> {
        let decoded = wallpaper::decode(source)?;
        let palette = palette::extract(&decoded);
        let rgb = wallpaper::opaque(decoded);
        let (w, h) = rgb.dimensions();
        let scale = (1000.0 / w.max(h) as f32).min(1.0);
        let thumbnail = image::imageops::thumbnail(
            &rgb,
            (w as f32 * scale).round().max(1.0) as u32,
            (h as f32 * scale).round().max(1.0) as u32,
        );
        Ok(Self { thumbnail, palette })
    }
}

#[derive(Default)]
struct Photos {
    bases: HashMap<Wallpaper, Result<PhotoSample, String>>,
    previews: HashMap<String, Preview>,
}
impl Photos {
    fn invalidate(&mut self, source: &Wallpaper) {
        self.bases.remove(source);
        self.previews.retain(|_, p| &p.source != source);
    }
    fn sample(&mut self, source: &Wallpaper) -> Result<&PhotoSample, String> {
        self.bases
            .entry(source.clone())
            .or_insert_with(|| PhotoSample::load(source).map_err(|e| format!("{e:#}")))
            .as_ref()
            .map_err(Clone::clone)
    }
    fn palette(&mut self, source: &Wallpaper) -> Result<Palette, String> {
        self.sample(source)?.palette.clone().ok_or_else(|| {
            "No visible colors found. Choose a wallpaper with visible pixels.".into()
        })
    }
    fn get(&mut self, ctx: &eframe::egui::Context, t: &Theme) -> Result<TextureHandle, String> {
        if let Some(p) = self.previews.get(&t.id)
            && p.source == t.wallpaper
            && p.effects == t.effects
        {
            return Ok(p.texture.clone());
        }
        let base = &self.sample(&t.wallpaper)?.thumbnail;
        let pixels = if t.effects == Effects::default() {
            rgb_image(base)
        } else {
            rgb_image(&wallpaper::render(base, t.effects))
        };
        let texture = ctx.load_texture(&t.id, pixels, TextureOptions::LINEAR);
        self.previews.insert(
            t.id.clone(),
            Preview {
                source: t.wallpaper.clone(),
                effects: t.effects,
                texture: texture.clone(),
            },
        );
        Ok(texture)
    }
}
fn rgb_image(image: &image::RgbImage) -> ColorImage {
    ColorImage::from_rgb(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    )
}

struct Draft {
    theme: Theme,
    original: Theme,
}
pub struct App {
    store: Store,
    library: Library,
    selected: String,
    page: Page,
    filter: Filter,
    photos: Photos,
    draft: Option<Draft>,
    replace: Option<Theme>,
    show_code: bool,
    message: Option<(String, String)>,
    report: Option<(String, apply::Report)>,
    job: Option<(String, Receiver<Result<apply::Report>>)>,
}

impl App {
    pub fn new(store: Store, library: Library) -> Self {
        Self {
            store,
            library,
            selected: "canopy".into(),
            page: Page::Library,
            filter: Filter::All,
            photos: Photos::default(),
            draft: None,
            replace: None,
            show_code: false,
            message: None,
            report: None,
            job: None,
        }
    }
    fn themes(&self) -> Vec<Theme> {
        Photo::ALL
            .into_iter()
            .map(Theme::preset)
            .chain(self.library.themes.iter().cloned())
            .collect()
    }
    fn current(&self) -> Theme {
        self.themes()
            .into_iter()
            .find(|t| t.id == self.selected)
            .unwrap_or_else(|| Theme::preset(Photo::Canopy))
    }
    fn start_edit(&mut self, new: bool) {
        let mut theme = self.current();
        if new || !theme.id.starts_with("custom-") {
            theme.id = format!("custom-{}", crate::unique_id());
            theme.name = if new {
                "Untitled theme".into()
            } else {
                format!("{} copy", theme.name)
            };
        }
        self.request_edit(theme);
    }

    fn theme_from_wallpaper(&mut self, path: PathBuf) {
        let source = Wallpaper::Local(path.clone());
        self.photos.invalidate(&source);
        match self.photos.palette(&source) {
            Ok(palette) => self.request_edit(Theme::from_wallpaper(&path, palette)),
            Err(detail) => self.message = Some(("Could not create theme".into(), detail)),
        }
    }

    fn request_edit(&mut self, theme: Theme) {
        if self.draft.as_ref().is_some_and(|d| d.theme != d.original) {
            self.replace = Some(theme);
        } else {
            self.edit(theme);
        }
    }
    fn edit(&mut self, t: Theme) {
        self.draft = Some(Draft {
            original: t.clone(),
            theme: t,
        });
        self.page = Page::Editor;
    }
    fn save(&mut self) -> Result<()> {
        let mut t = self
            .draft
            .as_ref()
            .context("Select a theme to edit")?
            .theme
            .clone();
        t.name = t.name.trim().into();
        t.code_theme = t.code_theme.trim().into();
        t.vim_scheme = t.vim_scheme.trim().into();
        t.validate()?;
        if let Wallpaper::Local(path) = &t.wallpaper {
            if path.parent() != Some(self.store.root.join("images").as_path()) {
                t.wallpaper = Wallpaper::Local(wallpaper::import(&self.store.root, &t.wallpaper)?);
            } else {
                wallpaper::decode(&t.wallpaper)?;
            }
        }
        let mut library = self.library.clone();
        if let Some(old) = library.themes.iter_mut().find(|old| old.id == t.id) {
            *old = t.clone();
        } else {
            library.themes.push(t.clone());
        }
        self.store.save(&library)?;
        self.library = library;
        self.selected = t.id;
        self.draft = None;
        self.page = Page::Library;
        self.filter = Filter::All;
        Ok(())
    }
    fn apply(&mut self, ctx: &eframe::egui::Context, t: Theme) {
        if self.job.is_some() {
            return;
        }
        let root = self.store.root.clone();
        let (tx, rx) = mpsc::channel();
        self.job = Some((t.name.clone(), rx));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(apply::run(&root, &t));
            ctx.request_repaint();
        });
    }
    fn header(&mut self, ctx: &eframe::egui::Context) {
        TopBottomPanel::top("masthead")
            .frame(
                Frame::new()
                    .fill(WHITE)
                    .stroke(Stroke::new(1.0_f32, LINE))
                    .inner_margin(Margin::symmetric(30, 0)),
            )
            .show(ctx, |ui| {
                ui.set_height(68.0);
                ui.horizontal_centered(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                    logo(ui.painter(), mark);
                    ui.label(title("WTE", 28.0));
                    ui.add_space(2.0);
                    ui.label(muted("Windows Theme Engine"));
                    ui.add_space(22.0);
                    for (page, label) in
                        [(Page::Library, "Library"), (Page::Editor, "Theme editor")]
                    {
                        let active = self.page == page;
                        let resp = ui.add(
                            Button::new(RichText::new(label).font(strong(14.5)).color(if active {
                                GREEN
                            } else {
                                MUTED
                            }))
                            .fill(if active {
                                Color32::from_rgb(232, 239, 227)
                            } else {
                                WHITE
                            })
                            .stroke(Stroke::NONE)
                            .corner_radius(5),
                        );
                        if resp.clicked() {
                            if page == Page::Editor && self.draft.is_none() {
                                self.start_edit(false);
                            } else {
                                self.page = page;
                            }
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, "+   New theme", true).clicked() {
                            self.start_edit(true);
                        }
                    });
                });
            });
    }
    fn library_ui(&mut self, ui: &mut Ui) {
        let selected = self.current();
        ui.horizontal(|ui| {
            ui.label(title("Theme library", 37.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, "Theme from wallpaper…", false).clicked()
                    && let Some(path) = pick_wallpaper()
                {
                    self.theme_from_wallpaper(path);
                }
            });
        });
        ui.add_space(11.0);
        let width = ui.available_width();
        let h = if width < 870.0 { 245.0 } else { 272.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
        let split = rect.left() + width * 0.61;
        let left = Rect::from_min_max(rect.min, pos2(split, rect.bottom()));
        let right = Rect::from_min_max(pos2(split, rect.top()), rect.max);
        let rounding = CornerRadius {
            nw: 10,
            sw: 10,
            ne: 0,
            se: 0,
        };
        self.preview(ui, left, &selected, rounding, true);
        let p = ui.painter();
        p.rect_filled(
            right,
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: 10,
                se: 10,
            },
            INK,
        );
        let inner = right.shrink2(vec2(24.0, 18.0));
        let mut info = ui.new_child(
            UiBuilder::new()
                .id_salt("details")
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        info.spacing_mut().item_spacing.y = 5.0;
        let builtin = !selected.id.starts_with("custom-");
        info.label(
            RichText::new(if builtin {
                "PRESET THEME"
            } else {
                "CUSTOM THEME"
            })
            .monospace()
            .size(11.5)
            .color(LIME),
        );
        info.add(
            Label::new(
                RichText::new(&selected.name)
                    .font(strong(35.0))
                    .color(WHITE),
            )
            .truncate(),
        );
        info.add_space(6.0);
        let palette_width = info.available_width().min(245.0);
        let (sw, _) = info.allocate_exact_size(vec2(palette_width, 17.0), Sense::hover());
        swatches(info.painter(), sw, selected.palette.colors());
        info.label(
            RichText::new(format!("Accent  {}", hex(selected.palette.accent)))
                .monospace()
                .size(12.0)
                .color(Color32::from_rgb(192, 207, 190)),
        );
        info.add_space(7.0);
        let components = [
            (selected.desktop, "Windows"),
            (selected.vscode, "VS Code"),
            (selected.nvim, "Neovim"),
            (selected.glaze, "GlazeWM"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect::<Vec<_>>()
        .join("  ·  ");
        info.label(RichText::new(components).size(13.0).color(WHITE));
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 37.0), inner.max);
        let mut actions = ui.new_child(
            UiBuilder::new()
                .max_rect(buttons)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let label = if self.job.is_some() {
            "Applying…"
        } else {
            "Apply theme"
        };
        let apply = actions.add_enabled(
            self.job.is_none(),
            Button::new(RichText::new(label).font(strong(15.0)).color(INK))
                .fill(LIME)
                .min_size(vec2(105.0, 35.0))
                .corner_radius(5),
        );
        let apply = if selected.vscode && selected.code_theme.trim().is_empty() {
            apply.on_hover_text(format!(
                "VS Code uses {}. On first use WTE installs the bundled theme through VS Code.",
                crate::exovoid::NAME
            ))
        } else {
            apply
        };
        if apply.clicked() {
            self.apply(ui.ctx(), selected.clone());
        }
        if actions
            .add(
                Button::new(RichText::new("Edit theme").font(strong(14.0)).color(WHITE))
                    .stroke(Stroke::new(1.0_f32, MUTED))
                    .fill(INK)
                    .min_size(vec2(84.0, 35.0))
                    .corner_radius(5),
            )
            .clicked()
        {
            self.start_edit(false);
        }
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(title("Collection", 23.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (f, label) in [
                    (Filter::Custom, "Custom"),
                    (Filter::Presets, "Presets"),
                    (Filter::All, "All themes"),
                ] {
                    let active = f == self.filter;
                    let resp = ui.add(
                        Button::new(RichText::new(label).size(13.0).color(if active {
                            WHITE
                        } else {
                            MUTED
                        }))
                        .fill(if active { GREEN } else { PAPER })
                        .stroke(if active {
                            Stroke::NONE
                        } else {
                            Stroke::new(1.0_f32, LINE)
                        })
                        .corner_radius(15),
                    );
                    if resp.clicked() {
                        self.filter = f;
                    }
                }
            });
        });
        ui.add_space(6.0);
        let themes: Vec<_> = self
            .themes()
            .into_iter()
            .filter(|t| match self.filter {
                Filter::All => true,
                Filter::Custom => t.id.starts_with("custom-"),
                Filter::Presets => !t.id.starts_with("custom-"),
            })
            .collect();
        if themes.is_empty() {
            Frame::new()
                .fill(WHITE)
                .inner_margin(18)
                .corner_radius(7)
                .show(ui, |ui| {
                    ui.label(title("Your custom collection is empty", 18.0));
                    if button(ui, "Create a theme", true).clicked() {
                        self.start_edit(true);
                    }
                });
            return;
        }
        let cols = if width < 800.0 { 2 } else { 3 };
        let card_width = (width - ui.spacing().item_spacing.x * (cols - 1) as f32) / cols as f32;
        for row in themes.chunks(cols) {
            ui.horizontal(|ui| {
                for t in row {
                    let (r, resp) = ui.allocate_exact_size(vec2(card_width, 159.0), Sense::click());
                    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &t.name));
                    ui.painter().rect_filled(r, 7, WHITE);
                    let pic = Rect::from_min_max(
                        r.min + vec2(1.0, 1.0),
                        pos2(r.right() - 1.0, r.bottom() - 50.0),
                    );
                    self.preview(
                        ui,
                        pic,
                        t,
                        CornerRadius {
                            nw: 7,
                            ne: 7,
                            sw: 0,
                            se: 0,
                        },
                        false,
                    );
                    let active = t.id == self.selected;
                    let p = ui
                        .painter()
                        .with_clip_rect(r.shrink(2.0).intersect(ui.clip_rect()));
                    let y = r.bottom() - 26.0;
                    let text_rect = Rect::from_min_max(
                        pos2(r.left() + 12.0, y - 15.0),
                        pos2(r.right() - 91.0, y + 16.0),
                    );
                    p.with_clip_rect(text_rect.intersect(ui.clip_rect())).text(
                        pos2(text_rect.left(), y),
                        Align2::LEFT_CENTER,
                        &t.name,
                        strong(17.0),
                        INK,
                    );
                    for (i, c) in [t.palette.surface, t.palette.accent, t.palette.secondary]
                        .into_iter()
                        .enumerate()
                    {
                        p.circle_filled(pos2(r.right() - 59.0 + i as f32 * 18.0, y), 6.0, color(c));
                    }
                    ui.painter().rect_stroke(
                        r,
                        7,
                        Stroke::new(
                            if active { 2.0_f32 } else { 1.0_f32 },
                            if active {
                                GREEN
                            } else if resp.hovered() {
                                MUTED
                            } else {
                                LINE
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    if active {
                        let badge = Rect::from_min_size(r.min + vec2(9.0, 9.0), vec2(65.0, 20.0));
                        p.rect_filled(badge, 10, WHITE);
                        p.text(
                            badge.center(),
                            Align2::CENTER_CENTER,
                            "Selected",
                            strong(11.0),
                            GREEN,
                        );
                    }
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        self.selected = t.id.clone();
                    }
                }
            });
        }
    }
    fn preview(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        t: &Theme,
        radius: CornerRadius,
        controls: bool,
    ) {
        match self.photos.get(ui.ctx(), t) {
            Ok(texture) => photo(ui.painter(), rect, &texture, radius),
            Err(error) => {
                ui.painter().rect_filled(rect, radius, LINE);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Wallpaper unavailable",
                    FontId::proportional(15.0),
                    INK,
                );
                ui.interact(rect, ui.id().with((&t.id, "error")), Sense::hover())
                    .on_hover_text(error);
            }
        }
        if !controls {
            return;
        }
        if self.show_code {
            editor_sample(ui.painter(), rect, t);
        }
        let labels = Rect::from_min_size(rect.min + vec2(14.0, 13.0), vec2(154.0, 29.0));
        ui.painter().rect_filled(labels, 5, INK);
        for (i, name) in ["Wallpaper", "Editor"].into_iter().enumerate() {
            let r = Rect::from_min_max(
                pos2(labels.left() + [3.0, 88.0][i], labels.top() + 3.0),
                pos2(labels.left() + [86.0, 151.0][i], labels.bottom() - 3.0),
            );
            let active = self.show_code == (i == 1);
            if active {
                ui.painter().rect_filled(r, 3, WHITE);
            }
            ui.painter().text(
                r.center(),
                Align2::CENTER_CENTER,
                name,
                strong(12.5),
                if active { INK } else { WHITE },
            );
            let resp = ui.interact(r, ui.id().with(("preview-mode", i)), Sense::click());
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                self.show_code = i == 1;
            }
        }
        let byline = match &t.wallpaper {
            Wallpaper::Bundled(p) => format!("Photo by {}", p.credit()),
            Wallpaper::Local(_) => String::from("Custom wallpaper"),
        };
        let pos = rect.left_bottom() + vec2(14.0, -12.0);
        let galley = ui
            .painter()
            .layout_no_wrap(byline, FontId::proportional(12.0), WHITE);
        let box_rect = Rect::from_min_size(
            pos - vec2(0.0, galley.size().y + 8.0),
            galley.size() + vec2(14.0, 8.0),
        );
        ui.painter()
            .rect_filled(box_rect, 3, Color32::from_black_alpha(156));
        ui.painter()
            .galley(box_rect.min + vec2(7.0, 4.0), galley, WHITE);
    }
    fn editor_ui(&mut self, ui: &mut Ui) {
        let Some(mut draft) = self.draft.take() else {
            self.page = Page::Library;
            return;
        };
        let t = &mut draft.theme;
        let mut save = false;
        ui.horizontal(|ui| {
            ui.label(title("Theme editor", 37.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                save = button(ui, "Save theme", true).clicked();
            });
        });
        ui.add_space(12.0);
        ui.columns(2,|cols|{
            let left=&mut cols[0];
            let w=left.available_width();
            let (r,_)=left.allocate_exact_size(vec2(w,(w*0.58).clamp(190.0,330.0)),Sense::hover());
            self.preview(left,r,t,CornerRadius::same(8),true);
            section(left,"Wallpaper",|ui|{
                let selected=match &t.wallpaper { Wallpaper::Bundled(p)=>p.title().to_owned(), Wallpaper::Local(p)=>p.file_name().unwrap_or_default().to_string_lossy().into_owned() };
                ComboBox::from_id_salt("wallpicker").selected_text(selected).width((ui.available_width()-10.0).max(80.0)).show_ui(ui,|ui|{
                    for p in Photo::ALL {ui.selectable_value(&mut t.wallpaper,Wallpaper::Bundled(p),p.title());}
                });
                if button(ui,"Choose local image…",false).clicked()
                    && let Some(path)=pick_wallpaper() {
                    let source=Wallpaper::Local(path);
                    self.photos.invalidate(&source);
                    match self.photos.sample(&source) {
                        Ok(_)=>t.wallpaper=source,
                        Err(detail)=>self.message=Some(("Could not open wallpaper".into(),detail)),
                    }
                }
            });
            section(left,"Wallpaper effects",|ui|{
                effect(ui,"Blur",&mut t.effects.blur);
                effect(ui,"Raindrops",&mut t.effects.rain);
                effect(ui,"Film grain",&mut t.effects.grain);
            });
            let right=&mut cols[1];
            section(right,"Theme name",|ui|{
                ui.add(TextEdit::singleline(&mut t.name).background_color(PAPER).margin(Margin::symmetric(7,5)).desired_width(f32::INFINITY).char_limit(64).hint_text("Theme name"));
            });
            section(right,"Color palette",|ui|{
                let p=&mut t.palette;
                Grid::new("palette-editor").num_columns(2).spacing(vec2(16.0,7.0)).show(ui,|ui|{
                    for (i,(label,value)) in [("Background",&mut p.background),("Surface",&mut p.surface),("Text",&mut p.text),("Muted",&mut p.muted),("Accent",&mut p.accent),("Secondary",&mut p.secondary)].into_iter().enumerate() {
                        ui.horizontal(|ui|{ui.color_edit_button_srgb(value).on_hover_text(hex(*value));ui.label(label);});
                        if i%2==1 {ui.end_row();}
                    }
                });
                if button(ui,"Use wallpaper colors",false).clicked() {
                    match self.photos.palette(&t.wallpaper) {
                        Ok(palette)=>t.palette=palette,
                        Err(detail)=>self.message=Some(("Could not detect wallpaper colors".into(),detail)),
                    }
                }
            });
            section(right,"Components",|ui|{
                ui.checkbox(&mut t.desktop,"Windows wallpaper and accent");
                ui.checkbox(&mut t.vscode,"Visual Studio Code");
                if t.vscode{ui.add(TextEdit::singleline(&mut t.code_theme).background_color(PAPER).margin(Margin::symmetric(7,5)).hint_text("ExoVoid, or installed VS Code theme").desired_width(f32::INFINITY)).on_hover_text("Leave blank to use ExoVoid syntax with your palette. WTE installs the bundled ExoVoid theme through VS Code on first use.");}
                if t.vscode && t.code_theme.trim().is_empty() {
                    ui.label(muted("First use installs the bundled ExoVoid theme in VS Code if needed."));
                }
                ui.checkbox(&mut t.nvim,"Neovim");
                if t.nvim{ui.add(TextEdit::singleline(&mut t.vim_scheme).background_color(PAPER).margin(Margin::symmetric(7,5)).hint_text("ExoVoid syntax, or installed colorscheme").desired_width(f32::INFINITY)).on_hover_text("Leave blank to use matching ExoVoid syntax with your palette.");}
                ui.checkbox(&mut t.glaze,"GlazeWM borders");
                if t.glaze {
                    border_picker(ui,"Focused",t.palette.accent,&mut t.focused);
                    border_picker(ui,"Unfocused",t.palette.surface,&mut t.unfocused);
                }
            });
        });
        self.draft = Some(draft);
        if save && let Err(e) = self.save() {
            self.message = Some(("Could not save theme".into(), format!("{e:#}")));
        }
    }
    fn dialogs(&mut self, ctx: &eframe::egui::Context) {
        if let Some((name, rx)) = &self.job {
            match rx.try_recv() {
                Ok(Ok(report)) => {
                    self.report = Some((name.clone(), report));
                    self.job = None;
                }
                Ok(Err(e)) => {
                    self.message = Some(("Theme application".into(), format!("{e:#}")));
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = Some((
                        "Theme application interrupted".into(),
                        "Check the component settings before retrying.".into(),
                    ));
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some((label, detail)) = &self.message {
            let close = Modal::new(Id::new("message")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(title(label, 22.0));
                ui.label(detail);
                button(ui, "Close", true).clicked()
            });
            if close.inner || close.should_close() {
                self.message = None;
            }
        } else if let Some((name, report)) = &self.report {
            let close = Modal::new(Id::new("report")).show(ctx, |ui| {
                ui.set_width(470.0);
                ui.label(title(format!("Apply results · {name}"), 22.0));
                for entry in &report.entries {
                    ui.separator();
                    ui.label(title(entry.component, 15.0));
                    ui.colored_label(
                        if entry.error {
                            Color32::from_rgb(153, 49, 39)
                        } else {
                            MUTED
                        },
                        &entry.detail,
                    );
                }
                button(ui, "Done", true).clicked()
            });
            if close.inner || close.should_close() {
                self.report = None;
            }
        } else if self.replace.is_some() {
            let modal = Modal::new(Id::new("replace-draft")).show(ctx, |ui| {
                ui.set_width(350.0);
                ui.label(title("Replace unsaved edits?", 21.0));
                ui.horizontal(|ui| {
                    if button(ui, "Keep editing", false).clicked() {
                        self.replace = None;
                        self.page = Page::Editor;
                    }
                    if button(ui, "Discard edits", true).clicked()
                        && let Some(t) = self.replace.take()
                    {
                        self.edit(t);
                    }
                });
            });
            if modal.should_close() {
                self.replace = None;
            }
        }
    }
}

fn pick_wallpaper() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Choose a wallpaper")
        .add_filter("Photo", &["jpg", "jpeg", "png"])
        .pick_file()
}

fn section(ui: &mut Ui, name: &str, body: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(WHITE)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(7)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(title(name, 17.0));
            ui.add_space(2.0);
            body(ui);
        });
    ui.add_space(3.0);
}
fn effect(ui: &mut Ui, label: &str, value: &mut u8) {
    ui.horizontal(|ui| {
        ui.add_sized([76.0, 20.0], Label::new(label).halign(Align::Min));
        ui.spacing_mut().slider_width = (ui.available_width() - 55.0).clamp(70.0, 235.0);
        ui.add(Slider::new(value, 0..=100).suffix("%"))
            .on_hover_text(label);
    });
}
fn border_picker(
    ui: &mut Ui,
    label: &str,
    base: crate::theme::Rgb,
    value: &mut Option<crate::theme::Rgb>,
) {
    ui.horizontal(|ui| {
        ui.label(muted(label));
        let mut linked = value.is_none();
        if ui.checkbox(&mut linked, "Use palette").changed() {
            *value = if linked { None } else { Some(base) };
        }
        if let Some(rgb) = value {
            ui.color_edit_button_srgb(rgb);
        } else {
            ui.label(small(hex(base)));
        }
    });
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Keep the library lock until the in-flight apply finishes even if the window closes.
        if let Some((_, job)) = self.job.take() {
            let _ = job.recv();
        }
    }

    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.header(ctx);
        CentralPanel::default()
            .frame(Frame::new().fill(PAPER))
            .show(ctx, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin::symmetric(30, 21))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match self.page {
                                Page::Library => self.library_ui(ui),
                                Page::Editor => self.editor_ui(ui),
                            }
                        });
                });
            });
        self.dialogs(ctx);
    }
}
