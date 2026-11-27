use crate::theme::Rgb;
use eframe::egui::*;

pub const PAPER: Color32 = Color32::from_rgb(243, 244, 237);
pub const WHITE: Color32 = Color32::from_rgb(252, 252, 247);
pub const INK: Color32 = Color32::from_rgb(27, 40, 33);
pub const MUTED: Color32 = Color32::from_rgb(83, 100, 89);
pub const LINE: Color32 = Color32::from_rgb(203, 212, 199);
pub const GREEN: Color32 = Color32::from_rgb(40, 77, 56);
pub const LIME: Color32 = Color32::from_rgb(213, 236, 169);
pub fn color([r, g, b]: Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}
pub fn title(s: impl Into<String>, size: f32) -> RichText {
    RichText::new(s).font(strong(size)).color(INK)
}
pub fn small(s: impl Into<String>) -> RichText {
    RichText::new(s).monospace().size(11.5).color(MUTED)
}
pub fn muted(s: impl Into<String>) -> RichText {
    RichText::new(s).color(MUTED).size(14.0)
}
pub fn button(ui: &mut Ui, label: &str, primary: bool) -> Response {
    let (bg, fg, outline) = if primary {
        (GREEN, Color32::WHITE, GREEN)
    } else {
        (WHITE, INK, LINE)
    };
    ui.add(
        Button::new(RichText::new(label).font(strong(14.0)).color(fg))
            .fill(bg)
            .stroke(Stroke::new(1.0_f32, outline))
            .corner_radius(6)
            .min_size(vec2(0.0, 35.0)),
    )
}

pub fn setup(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "dm".into(),
        FontData::from_static(include_bytes!("../assets/fonts/DM-Sans-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "dm-bold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/DM-Sans-Semibold.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "dm".into());
    let mut bold = fonts.families[&FontFamily::Proportional].clone();
    bold.insert(0, "dm-bold".into());
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), bold);
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = Visuals::light();
    style.visuals.panel_fill = PAPER;
    style.visuals.window_fill = WHITE;
    style.visuals.extreme_bg_color = WHITE;
    style.visuals.override_text_color = Some(INK);
    style.visuals.selection.bg_fill = GREEN;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, WHITE);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(222, 230, 218);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(209, 224, 202);
    style.visuals.widgets.active.bg_fill = LIME;
    style.spacing.item_spacing = vec2(9.0, 7.0);
    style.spacing.button_padding = vec2(13.0, 6.0);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    ctx.set_style(style);
}

pub fn logo(p: &Painter, r: Rect) {
    p.rect_filled(r, 7, GREEN);
    let s = r.width() / 36.0;
    for (i, c) in [
        LIME,
        Color32::from_rgb(162, 201, 138),
        Color32::from_rgb(112, 165, 115),
    ]
    .into_iter()
    .enumerate()
    .rev()
    {
        let x = 5.5 + 6.6 * i as f32;
        let points = [(x, 12.0), (x + 5.9, 9.0), (x + 11.1, 23.5), (x + 5.1, 27.0)]
            .map(|(a, b)| r.min + vec2(a * s, b * s));
        p.add(Shape::convex_polygon(points.to_vec(), c, Stroke::NONE));
    }
}

pub fn photo(p: &Painter, rect: Rect, texture: &TextureHandle, radius: CornerRadius) {
    let [w, h] = texture.size();
    let src = w as f32 / h as f32;
    let dest = rect.aspect_ratio();
    let extent = if src > dest {
        vec2(dest / src, 1.0)
    } else {
        vec2(1.0, src / dest)
    };
    let uv = Rect::from_center_size(pos2(0.5, 0.5), extent);
    p.add(epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(texture.id(), uv));
}

pub fn swatches(p: &Painter, rect: Rect, colors: [Rgb; 6]) {
    let gap = 3.0;
    let w = (rect.width() - 5.0 * gap) / 6.0;
    for (i, c) in colors.into_iter().enumerate() {
        let r = Rect::from_min_size(
            rect.min + vec2(i as f32 * (w + gap), 0.0),
            vec2(w, rect.height()),
        );
        p.rect_filled(r, 2, color(c));
        p.rect_stroke(
            r,
            2,
            Stroke::new(0.7_f32, Color32::from_black_alpha(26)),
            StrokeKind::Inside,
        );
    }
}

pub fn editor_sample(p: &Painter, rect: Rect, t: &crate::theme::Theme) {
    let q = &t.palette;
    let r = Rect::from_center_size(rect.center(), vec2((rect.width() - 48.0).min(355.0), 139.0));
    p.rect_filled(
        r.translate(vec2(0.0, 5.0)),
        7,
        Color32::from_black_alpha(65),
    );
    p.rect_filled(r, 7, color(q.background));
    p.rect_stroke(
        r,
        7,
        Stroke::new(2.0_f32, color(t.focused.unwrap_or(q.accent))),
        StrokeKind::Inside,
    );
    let inner = r.shrink(2.0);
    p.rect_filled(
        Rect::from_min_size(inner.min, vec2(inner.width(), 27.0)),
        4,
        color(q.surface),
    );
    p.circle_filled(inner.min + vec2(13.0, 14.0), 3.5, color(q.accent));
    p.text(
        inner.min + vec2(23.0, 14.0),
        Align2::LEFT_CENTER,
        "palette.rs",
        FontId::monospace(11.0),
        color(crate::exovoid::rgb(crate::exovoid::workbench(
            "tab.activeForeground",
        ))),
    );
    let plain = crate::exovoid::workbench("editor.foreground");
    let function = crate::exovoid::syntax("entity.name.function");
    let lines: &[&[(&str, &str)]] = &[
        &[("// Desktop palette", crate::exovoid::syntax("comment"))],
        &[
            ("fn", crate::exovoid::syntax("keyword")),
            (" ", plain),
            ("main", function),
            ("() {", plain),
        ],
        &[
            ("    ", plain),
            ("apply", function),
            ("(", plain),
            ("\"theme\"", crate::exovoid::syntax("string")),
            (");", plain),
        ],
        &[("}", plain)],
    ];
    for (i, spans) in lines.iter().enumerate() {
        let y = inner.top() + 40.0 + 21.0 * i as f32;
        p.text(
            pos2(inner.left() + 14.0, y),
            Align2::LEFT_CENTER,
            (i + 1).to_string(),
            FontId::monospace(11.0),
            color(crate::exovoid::rgb(crate::exovoid::workbench(
                "editorLineNumber.foreground",
            ))),
        );
        let mut job = epaint::text::LayoutJob::default();
        for &(token, hex) in *spans {
            job.append(
                token,
                0.0,
                TextFormat::simple(FontId::monospace(13.0), color(crate::exovoid::rgb(hex))),
            );
        }
        let line = p.layout_job(job);
        p.galley(
            pos2(inner.left() + 38.0, y - line.size().y / 2.0),
            line,
            Color32::WHITE,
        );
    }
}
