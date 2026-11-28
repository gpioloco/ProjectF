use crate::{
    apply,
    design::*,
    palette,
    store::{Library, Store},
    theme::{Appearance, Effects, Palette, Photo, Theme, Wallpaper, hex},
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
enum PickFor {
    NewTheme,
    Draft(String),
}
type PickResult = Result<Option<(PathBuf, PhotoSample)>>;

pub struct App {
    picker: rfd::FileDialog,
    confirm_restore: bool,
    pending_photo: Option<(PickFor, Receiver<PickResult>)>,
    store: Store,
    library: Library,
    styled: Option<(Appearance, Palette)>,
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
    pub fn new(store: Store, library: Library, picker: rfd::FileDialog) -> Self {
        Self {
            picker,
            pending_photo: None,
            confirm_restore: false,
            store,
            library,
            styled: None,
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
        self.library
            .themes
            .iter()
            .find(|t| t.id == self.library.selected)
            .cloned()
            .or_else(|| {
                Photo::ALL
                    .into_iter()
                    .map(Theme::preset)
                    .find(|t| t.id == self.library.selected)
            })
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

    fn choose_wallpaper(&mut self, ctx: &eframe::egui::Context, target: PickFor) {
        if self.pending_photo.is_some() {
            return;
        }
        let dialog = self.picker.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending_photo = Some((target, rx));
        std::thread::spawn(move || {
            // On Windows rfd initializes this worker as a COM STA. The window's
            // thread continues pumping messages for the owned Explorer dialog.
            let result = dialog
                .pick_file()
                .map(|path| {
                    let sample = PhotoSample::load(&Wallpaper::Local(path.clone()))?;
                    Ok((path, sample))
                })
                .transpose();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn finish_photo(&mut self) {
        let Some((_, rx)) = &self.pending_photo else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(anyhow::anyhow!("Wallpaper selection was interrupted."))
            }
        };
        let (target, _) = self.pending_photo.take().unwrap();
        match result {
            Ok(Some((path, sample))) => {
                let source = Wallpaper::Local(path.clone());
                let palette = sample.palette.clone();
                self.photos.invalidate(&source);
                self.photos.bases.insert(source.clone(), Ok(sample));
                match target {
                    PickFor::NewTheme => match palette {
                        Some(palette) => self.request_edit(Theme::from_wallpaper(&path, palette)),
                        None => {
                            self.message = Some((
                                "Could not create theme".into(),
                                "Choose a wallpaper with visible pixels.".into(),
                            ))
                        }
                    },
                    PickFor::Draft(id) => {
                        if let Some(draft) = &mut self.draft
                            && draft.theme.id == id
                        {
                            draft.theme.wallpaper = source;
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => self.message = Some(("Could not open wallpaper".into(), format!("{e:#}"))),
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
        library.selected = t.id;
        self.store.save(&library)?;
        self.library = library;
        self.draft = None;
        self.page = Page::Library;
        self.filter = Filter::All;
        Ok(())
    }
    fn start_job(
        &mut self,
        ctx: &eframe::egui::Context,
        title: String,
        action: impl FnOnce(&std::path::Path) -> Result<apply::Report> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        let root = self.store.root.clone();
        let (tx, rx) = mpsc::channel();
        self.job = Some((title, rx));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(action(&root));
            ctx.request_repaint();
        });
    }
    fn header(&mut self, ctx: &eframe::egui::Context, p: &Palette, margin: i8) {
        let style = ctx.style();
        TopBottomPanel::top("masthead")
            .frame(
                Frame::new()
                    .fill(style.visuals.window_fill)
                    .stroke(style.visuals.widgets.noninteractive.bg_stroke)
                    .inner_margin(Margin::symmetric(margin, 10)),
            )
            .show(ctx, |ui| {
                let narrow = ui.available_width() < 630.0;
                ui.horizontal(|ui| {
                    let (mark, _) = ui
                        .allocate_exact_size(vec2(CONTROL_HEIGHT, CONTROL_HEIGHT), Sense::hover());
                    logo(ui.painter(), mark, p);
                    ui.label(title("WTE", 27.0));
                    if ui.available_width() > 880.0 {
                        ui.label(muted("Window Theme Engine"));
                    }
                    if !narrow {
                        self.navigation(ui, 120.0);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, "New theme", true).clicked() {
                            self.start_edit(true);
                        }
                        let mode = self.library.app_mode;
                        if button(ui, &format!("App · {}", mode.label()), false)
                            .on_hover_text("Switch the app between light and dark")
                            .clicked()
                        {
                            self.library.app_mode = if mode == Appearance::Light {
                                Appearance::Dark
                            } else {
                                Appearance::Light
                            };
                            if let Err(e) = self.store.save(&self.library) {
                                self.library.app_mode = mode;
                                self.message =
                                    Some(("Could not save appearance".into(), format!("{e:#}")));
                            }
                        }
                    });
                });
                if narrow {
                    let width = (ui.available_width() - ui.spacing().item_spacing.x) * 0.5;
                    ui.horizontal(|ui| self.navigation(ui, width));
                }
            });
    }

    fn navigation(&mut self, ui: &mut Ui, width: f32) {
        for (page, label) in [(Page::Library, "Library"), (Page::Editor, "Theme editor")] {
            if control(ui, label, self.page == page, width).clicked() {
                if page == Page::Editor && self.draft.is_none() {
                    self.start_edit(false);
                } else {
                    self.page = page;
                }
            }
        }
    }

    fn library_ui(&mut self, ui: &mut Ui, height: f32) {
        let bottom = ui.cursor().top() + height;
        let selected = self.current();
        let width = ui.available_width();
        let compact = width < 715.0;
        let action_width = if width < 630.0 {
            (width - ui.spacing().item_spacing.x) * 0.5
        } else {
            190.0
        };
        let actions = |ui: &mut Ui| {
            if control(ui, "Theme from wallpaper…", false, action_width)
                .on_hover_text("Create a theme from a wallpaper")
                .clicked()
            {
                self.choose_wallpaper(ui.ctx(), PickFor::NewTheme);
            }
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                if control(ui, "Restore", false, action_width)
                    .on_hover_text("Restore settings from before WTE applied a theme")
                    .clicked()
                {
                    self.confirm_restore = true;
                }
            });
        };
        page_heading(ui, "Theme library", actions);
        let h = if compact {
            (height * 0.60).max(400.0)
        } else {
            ((height - 100.0) * 0.61).max(236.0)
        };
        let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
        let (left, right, photo_corners, detail_corners) = if compact {
            let y = rect.bottom() - 201.0;
            (
                Rect::from_min_max(rect.min, pos2(rect.right(), y)),
                Rect::from_min_max(pos2(rect.left(), y), rect.max),
                CornerRadius {
                    nw: 10,
                    ne: 10,
                    sw: 0,
                    se: 0,
                },
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 10,
                    se: 10,
                },
            )
        } else {
            let split = rect.right() - (width * 0.36).clamp(316.0, 540.0);
            (
                Rect::from_min_max(rect.min, pos2(split, rect.bottom())),
                Rect::from_min_max(pos2(split, rect.top()), rect.max),
                CornerRadius {
                    nw: 10,
                    sw: 10,
                    ne: 0,
                    se: 0,
                },
                CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 10,
                    se: 10,
                },
            )
        };
        self.preview(ui, left, &selected, photo_corners, true);
        let palette = &selected.palette;
        let ink = color(palette::readable(palette.text, palette.background));
        let quiet = color(palette::readable(palette.muted, palette.background));
        ui.painter()
            .rect_filled(right, detail_corners, color(palette.background));
        ui.painter().rect_stroke(
            rect,
            10,
            ui.visuals().widgets.noninteractive.bg_stroke,
            StrokeKind::Inside,
        );
        let inner = right.shrink2(vec2(18.0, 14.0));
        let mut info = ui.new_child(
            UiBuilder::new()
                .id_salt("details")
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        info.spacing_mut().item_spacing.y = 5.0;
        let builtin = !selected.id.starts_with("custom-");
        info.label(
            RichText::new(format!(
                "{}  ·  {}",
                if builtin { "Preset" } else { "Custom" },
                selected.mode.label()
            ))
            .size(13.0)
            .color(quiet),
        );
        info.add(
            Label::new(title(&selected.name, if compact { 26.0 } else { 32.0 }).color(ink))
                .truncate(),
        )
        .on_hover_text(&selected.name);
        let (sw, _) = info.allocate_exact_size(vec2(info.available_width(), 16.0), Sense::hover());
        swatches(info.painter(), sw, selected.palette.colors());
        info.label(
            RichText::new(format!("Accent  {}", hex(selected.palette.accent)))
                .monospace()
                .size(12.0)
                .color(quiet),
        );
        let components = [
            (selected.desktop, "Windows"),
            (selected.vscode, "VS Code"),
            (selected.nvim, "Neovim"),
            (selected.glaze, "GlazeWM"),
            (selected.rillshift, "Rillshift"),
            (selected.terminal || selected.terminal_fit, "Terminal"),
            (selected.posh, "Oh My Posh"),
            (selected.chrome, "Chrome"),
            (selected.firefox, "Firefox"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect::<Vec<_>>()
        .join("  ·  ");
        info.label(RichText::new(components).size(13.0).color(ink));
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 37.0), inner.max);
        let mut actions = ui.new_child(
            UiBuilder::new()
                .max_rect(buttons)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let label = if self.job.is_some() {
            "Working…"
        } else {
            "Apply theme"
        };
        let action_width = (inner.width() - actions.spacing().item_spacing.x) * 0.5;
        let apply = actions
            .add_enabled_ui(self.job.is_none(), |ui| {
                control(ui, label, true, action_width)
            })
            .inner;
        let apply = if selected.vscode && selected.code_theme.trim().is_empty() {
            apply.on_hover_text(format!(
                "VS Code uses {}. On first use WTE installs the bundled theme through VS Code.",
                crate::exovoid::name(selected.mode)
            ))
        } else {
            apply
        };
        if apply.clicked() {
            let t = selected.clone();
            self.start_job(
                ui.ctx(),
                format!("Apply results · {}", t.name),
                move |root| apply::run(root, &t),
            );
        }
        if control(&mut actions, "Edit theme", false, action_width).clicked() {
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
                    let resp = control(ui, label, active, 96.0);
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
                .fill(ui.visuals().window_fill)
                .inner_margin(18)
                .corner_radius(7)
                .show(ui, |ui| {
                    ui.label(title("Your custom collection is empty", 18.0));
                    if control(ui, "Create a theme", true, 140.0).clicked() {
                        self.start_edit(true);
                    }
                });
            return;
        }
        let cols = ((width + 8.0) / 220.0).floor().max(1.0) as usize;
        let cols = cols.min(themes.len());
        let rows = themes.len().div_ceil(cols);
        let card_height =
            ((bottom - ui.cursor().top() - (rows - 1) as f32 * ui.spacing().item_spacing.y)
                / rows as f32)
                .max(135.0);
        let card_width = (width - ui.spacing().item_spacing.x * (cols - 1) as f32) / cols as f32;
        for row in themes.chunks(cols) {
            ui.horizontal(|ui| {
                for t in row {
                    let (r, resp) =
                        ui.allocate_exact_size(vec2(card_width, card_height), Sense::click());
                    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &t.name));
                    ui.painter().rect_filled(r, 7, ui.visuals().window_fill);
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
                    let active = t.id == self.library.selected;
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
                        ui.visuals().text_color(),
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
                                ui.visuals().hyperlink_color
                            } else if resp.hovered() {
                                ui.visuals().weak_text_color()
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    if active {
                        let badge = Rect::from_min_size(r.min + vec2(9.0, 9.0), vec2(65.0, 20.0));
                        p.rect_filled(badge, 10, ui.visuals().window_fill);
                        p.text(
                            badge.center(),
                            Align2::CENTER_CENTER,
                            "Selected",
                            strong(11.0),
                            ui.visuals().hyperlink_color,
                        );
                    }
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
                        && self.library.selected != t.id
                    {
                        self.library.selected = t.id.clone();
                        if let Err(e) = self.store.save(&self.library) {
                            self.message =
                                Some(("Could not save theme selection".into(), format!("{e:#}")));
                        }
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
                ui.painter().rect_filled(
                    rect,
                    radius,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Wallpaper unavailable",
                    FontId::proportional(15.0),
                    ui.visuals().text_color(),
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
        let labels = Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(188.0, CONTROL_HEIGHT));
        let mut switches = ui.new_child(
            UiBuilder::new()
                .max_rect(labels)
                .id_salt("preview-mode")
                .layout(Layout::left_to_right(Align::Center)),
        );
        switches.spacing_mut().item_spacing.x = 4.0;
        for (code, name) in [(false, "Wallpaper"), (true, "Editor")] {
            if control(&mut switches, name, self.show_code == code, 92.0).clicked() {
                self.show_code = code;
            }
        }
        let byline = match &t.wallpaper {
            Wallpaper::Bundled(p) => format!("Photo by {}", p.credit()),
            Wallpaper::Local(_) => String::from("Custom wallpaper"),
        };
        let pos = rect.left_bottom() + vec2(14.0, -12.0);
        let galley =
            ui.painter()
                .layout_no_wrap(byline, FontId::proportional(12.0), Color32::WHITE);
        let box_rect = Rect::from_min_size(
            pos - vec2(0.0, galley.size().y + 8.0),
            galley.size() + vec2(14.0, 8.0),
        );
        ui.painter()
            .rect_filled(box_rect, 3, Color32::from_black_alpha(156));
        ui.painter()
            .galley(box_rect.min + vec2(7.0, 4.0), galley, Color32::WHITE);
    }
    fn editor_ui(&mut self, ui: &mut Ui, height: f32) {
        let Some(mut draft) = self.draft.take() else {
            self.page = Page::Library;
            return;
        };
        let t = &mut draft.theme;
        let mut save = false;
        page_heading(ui, "Theme editor", |ui| {
            save = button(ui, "Save theme", true).clicked();
        });
        let width = ui.available_width();
        let gap = ui.spacing().item_spacing.x;
        if width < 785.0 {
            self.editor_photo(ui, t, (width * 0.55).max(210.0));
            self.wallpaper_section(ui, t);
            effects_section(ui, t);
            name_section(ui, t);
            self.palette_section(ui, t);
            components_section(ui, t);
        } else if width < 1310.0 {
            ui.columns(2, |cols| {
                self.editor_photo(&mut cols[0], t, (height - 377.0).max(235.0));
                self.wallpaper_section(&mut cols[0], t);
                effects_section(&mut cols[0], t);
                name_section(&mut cols[1], t);
                self.palette_section(&mut cols[1], t);
                components_section(&mut cols[1], t);
            });
        } else {
            let photo_width = (width - 2.0 * gap) * 0.44;
            let settings_width = (width - photo_width - 2.0 * gap) / 2.0;
            ui.horizontal_top(|ui| {
                column(ui, photo_width, |ui| {
                    self.editor_photo(ui, t, (height - 200.0).max(235.0));
                    self.wallpaper_section(ui, t);
                });
                column(ui, settings_width, |ui| {
                    name_section(ui, t);
                    self.palette_section(ui, t);
                    effects_section(ui, t);
                });
                column(ui, settings_width, |ui| components_section(ui, t));
            });
        }
        self.draft = Some(draft);
        if save && let Err(e) = self.save() {
            self.message = Some(("Could not save theme".into(), format!("{e:#}")));
        }
    }
    fn editor_photo(&mut self, ui: &mut Ui, t: &Theme, height: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        self.preview(ui, rect, t, CornerRadius::same(8), true);
    }
    fn wallpaper_section(&mut self, ui: &mut Ui, t: &mut Theme) {
        section(ui, "Wallpaper", |ui| {
            let selected = match &t.wallpaper {
                Wallpaper::Bundled(p) => p.title().to_owned(),
                Wallpaper::Local(p) => p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            };
            ui.spacing_mut().interact_size.y = CONTROL_HEIGHT;
            ComboBox::from_id_salt("wallpicker")
                .selected_text(selected)
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    for p in Photo::ALL {
                        ui.selectable_value(&mut t.wallpaper, Wallpaper::Bundled(p), p.title());
                    }
                });
            if control(ui, "Choose local image…", false, ui.available_width()).clicked() {
                self.choose_wallpaper(ui.ctx(), PickFor::Draft(t.id.clone()));
            }
        });
    }
    fn palette_section(&mut self, ui: &mut Ui, t: &mut Theme) {
        section(ui, "Color palette", |ui| {
            ui.horizontal(|ui| {
                ui.label("Theme mode");
                let mut mode = t.mode;
                let width = (ui.available_width() - ui.spacing().item_spacing.x) * 0.5;
                for value in [Appearance::Light, Appearance::Dark] {
                    if control(ui, value.label(), mode == value, width).clicked() {
                        mode = value;
                    }
                }
                t.set_mode(mode);
            });
            let p = &mut t.palette;
            ui.columns(2, |cols| {
                for (i, (label, value)) in [
                    ("Background", &mut p.background),
                    ("Surface", &mut p.surface),
                    ("Text", &mut p.text),
                    ("Muted", &mut p.muted),
                    ("Accent", &mut p.accent),
                    ("Secondary", &mut p.secondary),
                ]
                .into_iter()
                .enumerate()
                {
                    cols[i % 2].horizontal(|ui| {
                        ui.color_edit_button_srgb(value).on_hover_text(hex(*value));
                        ui.label(label);
                    });
                }
            });
            if control(ui, "Use wallpaper colors", false, ui.available_width()).clicked() {
                match self.photos.palette(&t.wallpaper) {
                    Ok(palette) => t.use_wallpaper_colors(palette),
                    Err(detail) => {
                        self.message = Some(("Could not detect wallpaper colors".into(), detail))
                    }
                }
            }
        });
    }
    fn dialogs(&mut self, ctx: &eframe::egui::Context) {
        if let Some((name, rx)) = &self.job {
            match rx.try_recv() {
                Ok(Ok(report)) => {
                    self.report = Some((name.clone(), report));
                    self.job = None;
                }
                Ok(Err(e)) => {
                    self.message = Some((name.clone(), format!("{e:#}")));
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = Some((
                        name.clone(),
                        "The operation was interrupted. Check the component settings before retrying.".into(),
                    ));
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.confirm_restore {
            let modal = Modal::new(Id::new("restore-confirm")).show(ctx, |ui| {
                ui.set_width(410.0);
                ui.label(title("Restore previous settings?", 22.0));
                ui.label("Restore each component to the state saved before its first change. WTE backs up files it replaces during restore. Your theme library stays available.");
                ui.horizontal(|ui| {
                    if control(ui, "Cancel", false, 145.0).clicked() { self.confirm_restore = false; }
                    if control(ui, "Restore settings", true, 145.0).clicked() {
                        self.confirm_restore = false;
                        self.start_job(ctx, "Restore results".into(), apply::restore);
                    }
                });
            });
            if modal.should_close() {
                self.confirm_restore = false;
            }
            return;
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
                ui.set_width((ctx.content_rect().width() - 48.0).min(470.0));
                ui.label(title(name, 22.0));
                ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 160.0).max(100.0))
                    .show(ui, |ui| {
                        for entry in &report.entries {
                            ui.separator();
                            ui.label(title(entry.component, 15.0));
                            ui.colored_label(
                                if entry.error {
                                    ui.visuals().error_fg_color
                                } else {
                                    ui.visuals().weak_text_color()
                                },
                                &entry.detail,
                            );
                            if let Some(path) = &entry.path {
                                ui.label(small(path.display().to_string()));
                                if button(ui, "Copy path", false).clicked() {
                                    ui.ctx().copy_text(path.to_string_lossy().into_owned());
                                }
                            }
                        }
                    });
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

fn page_heading(ui: &mut Ui, name: &str, actions: impl FnOnce(&mut Ui)) {
    if ui.available_width() < 630.0 {
        ui.label(title(name, 30.0));
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), CONTROL_HEIGHT),
            Layout::right_to_left(Align::Center),
            actions,
        );
    } else {
        ui.horizontal(|ui| {
            ui.label(title(name, 30.0));
            ui.with_layout(Layout::right_to_left(Align::Center), actions);
        });
    }
    ui.add_space(4.0);
}
fn column(ui: &mut Ui, width: f32, body: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(width);
        body(ui);
    });
}
fn name_section(ui: &mut Ui, t: &mut Theme) {
    section(ui, "Theme name", |ui| {
        ui.add(
            TextEdit::singleline(&mut t.name)
                .background_color(ui.visuals().panel_fill)
                .margin(Margin::symmetric(7, 5))
                .desired_width(f32::INFINITY)
                .char_limit(64)
                .hint_text("Theme name"),
        );
    });
}
fn effects_section(ui: &mut Ui, t: &mut Theme) {
    section(ui, "Wallpaper effects", |ui| {
        effect(ui, "Blur", &mut t.effects.blur);
        effect(ui, "Raindrops", &mut t.effects.rain);
        effect(ui, "Film grain", &mut t.effects.grain);
    });
}
fn components_section(ui: &mut Ui, t: &mut Theme) {
    section(ui, "Components", |ui| {
        ui.checkbox(&mut t.desktop, "Windows wallpaper, accent and mode");
        ui.checkbox(&mut t.vscode, "Visual Studio Code");
        if t.vscode {
            ui.add(TextEdit::singleline(&mut t.code_theme).background_color(ui.visuals().panel_fill).margin(Margin::symmetric(7,5)).hint_text("ExoVoid, or installed VS Code theme").desired_width(f32::INFINITY)).on_hover_text("Leave blank to use ExoVoid syntax with your palette. WTE installs the bundled ExoVoid theme through VS Code on first use.");
        }
        if t.vscode && t.code_theme.trim().is_empty() {
            ui.label(muted(
                "First use installs the bundled ExoVoid theme in VS Code if needed.",
            ));
        }
        ui.checkbox(&mut t.nvim, "Neovim");
        if t.nvim {
            ui.add(
                TextEdit::singleline(&mut t.vim_scheme)
                    .background_color(ui.visuals().panel_fill)
                    .margin(Margin::symmetric(7, 5))
                    .hint_text("ExoVoid syntax, or installed colorscheme")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("Leave blank to use matching ExoVoid syntax with your palette.");
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut t.chrome, "Chrome");
            ui.checkbox(&mut t.firefox, "Firefox");
        });
        if t.chrome {
            ui.label(muted("Load the Chrome folder once. Fully exit and reopen Chrome after switching palettes."))
                .on_hover_text("For live Windows accent changes, use Chrome’s default theme and enable Follow device colors in Chrome’s Appearance settings.");
        }
        if t.firefox {
            ui.label(muted(
                "Load the exported theme in Firefox using the path in Apply results.",
            ));
        }
        ui.checkbox(&mut t.terminal, "Windows Terminal colors");
        ui.checkbox(&mut t.terminal_fit, "Fit terminal to tiled windows")
                    .on_hover_text("Align inherited font metrics with the startup profile, remove padding, hide the scrollbar and allow smooth resizing. Restore recovers your original settings.");
        ui.checkbox(&mut t.posh, "Oh My Posh prompt");
        if t.posh {
            ui.label(muted("Point your shell’s Oh My Posh configuration at the exported file once. Apply results includes the path."));
        }
        ui.checkbox(&mut t.glaze, "GlazeWM border colors");
        ui.checkbox(&mut t.rillshift, "Rillshift borders and bar accent");
        if t.glaze || t.rillshift {
            border_picker(ui, "Focused", t.palette.accent, &mut t.focused);
            border_picker(ui, "Unfocused", t.palette.surface, &mut t.unfocused);
        }
    });
}
fn section(ui: &mut Ui, name: &str, body: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
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
        ui.allocate_ui_with_layout(
            vec2(76.0, 24.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(76.0);
                ui.label(label);
            },
        );
        ui.spacing_mut().slider_width = (ui.available_width() - 61.0).max(40.0);
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
        ui.allocate_ui_with_layout(
            vec2(68.0, 24.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(68.0);
                ui.label(muted(label));
            },
        );
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
        // Keep the library lock until an in-flight apply or restore finishes.
        if let Some((_, job)) = self.job.take() {
            let _ = job.recv();
        }
    }

    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.finish_photo();
        let theme = match (&self.page, &self.draft) {
            (Page::Editor, Some(draft)) => &draft.theme,
            _ => &self.current(),
        };
        let mode = self.library.app_mode;
        let palette = if theme.mode == mode {
            theme.palette.clone()
        } else {
            theme.palette.variant(mode)
        };
        if self
            .styled
            .as_ref()
            .is_none_or(|(old, p)| *old != mode || p != &palette)
        {
            if self.styled.as_ref().is_none_or(|(old, _)| *old != mode) {
                ctx.send_viewport_cmd(ViewportCommand::SetTheme(if mode == Appearance::Dark {
                    SystemTheme::Dark
                } else {
                    SystemTheme::Light
                }));
            }
            appearance(ctx, mode, &palette);
            self.styled = Some((mode, palette.clone()));
        }
        let margin = if ctx.content_rect().width() < 750.0 {
            14
        } else if ctx.content_rect().width() > 1450.0 {
            30
        } else {
            24
        };
        self.header(ctx, &palette, margin);
        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(Margin::symmetric(margin, 16)),
            )
            .show(ctx, |ui| {
                let height = ui.available_height();
                ScrollArea::vertical()
                    .id_salt(self.page == Page::Editor)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match self.page {
                            Page::Library => self.library_ui(ui, height),
                            Page::Editor => self.editor_ui(ui, height),
                        }
                    });
            });
        if self.pending_photo.is_some() {
            Modal::new(Id::new("opening-wallpaper")).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Opening wallpaper…");
                });
            });
        } else {
            self.dialogs(ctx);
        }
    }
}
