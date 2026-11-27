use crate::palette::{mix, readable};
use crate::theme::{Appearance, Palette, Rgb};
use eframe::egui::*;

pub const CONTROL_HEIGHT: f32 = 36.0;

pub fn color([r, g, b]: Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}
pub fn title(s: impl Into<String>, size: f32) -> RichText {
    RichText::new(s).font(strong(size))
}
pub fn small(s: impl Into<String>) -> RichText {
    RichText::new(s).monospace().size(11.5).weak()
}
pub fn muted(s: impl Into<String>) -> RichText {
    RichText::new(s).weak().size(14.0)
}
pub fn button(ui: &mut Ui, label: &str, primary: bool) -> Response {
    control(ui, label, primary, 120.0)
}

/// Buttons sharing a row use the same width and baseline.
pub fn control(ui: &mut Ui, label: &str, emphasized: bool, width: f32) -> Response {
    let v = ui.visuals();
    let (fill, ink, stroke) = if emphasized {
        (v.selection.bg_fill, v.selection.stroke.color, Stroke::NONE)
    } else {
        (
            v.window_fill,
            v.text_color(),
            v.widgets.noninteractive.bg_stroke,
        )
    };
    let response = ui
        .add_sized(
            [width, CONTROL_HEIGHT],
            Button::new(title(label, 14.0).color(ink))
                .fill(fill)
                .stroke(stroke)
                .corner_radius(6)
                .min_size(vec2(width, CONTROL_HEIGHT))
                .wrap_mode(TextWrapMode::Extend),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            6,
            Stroke::new(1.5_f32, ui.visuals().hyperlink_color),
            StrokeKind::Inside,
        );
    }
    response
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
}

pub fn appearance(ctx: &Context, mode: Appearance, palette: &Palette) {
    let dark = mode == Appearance::Dark;
    let theme = if dark { Theme::Dark } else { Theme::Light };
    ctx.set_theme(theme);
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    *v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let p = palette;
    v.panel_fill = color(p.background);
    v.window_fill = color(p.surface);
    v.extreme_bg_color = color(p.background);
    v.override_text_color = Some(color(readable(p.text, p.surface)));
    v.weak_text_color = Some(color(readable(p.muted, p.surface)));
    v.hyperlink_color = color(readable(p.accent, p.surface));
    v.selection.bg_fill = color(p.accent);
    v.selection.stroke = Stroke::new(1.0_f32, color(readable(p.text, p.accent)));
    let outline = Stroke::new(1.0_f32, color(mix(p.surface, p.muted, 0.40)));
    v.window_stroke = outline;
    v.widgets.noninteractive.bg_stroke = outline;
    v.widgets.noninteractive.fg_stroke.color = v.text_color();
    for (widget, amount) in [
        (&mut v.widgets.inactive, 0.12),
        (&mut v.widgets.hovered, 0.28),
        (&mut v.widgets.active, 0.42),
    ] {
        let fill = mix(p.surface, p.accent, amount);
        widget.bg_fill = color(fill);
        widget.weak_bg_fill = color(fill);
        widget.bg_stroke = outline;
        widget.fg_stroke.color = color(readable(p.text, fill));
    }
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.button_padding = vec2(10.0, 5.0);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    ctx.set_style_of(theme, style);
}

pub fn logo(p: &Painter, r: Rect, palette: &Palette) {
    p.rect_filled(r, 7, color(palette.accent));
    let s = r.width() / 36.0;
    let ink = readable(palette.text, palette.accent);
    for (i, c) in [
        color(ink),
        color(mix(ink, palette.accent, 0.24)),
        color(mix(ink, palette.accent, 0.48)),
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
    let ink = |source| color(crate::exovoid::ink(t.mode, source));
    let scale = (rect.width() / 630.0)
        .min((rect.height() - 100.0) / 139.0)
        .clamp(0.68, 1.65);
    let r = Rect::from_center_size(rect.center(), vec2(355.0, 139.0) * scale);
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
        Rect::from_min_size(inner.min, vec2(inner.width(), 27.0 * scale)),
        4,
        color(q.surface),
    );
    p.circle_filled(
        inner.min + vec2(13.0, 14.0) * scale,
        3.5 * scale,
        color(q.accent),
    );
    p.text(
        inner.min + vec2(23.0, 14.0) * scale,
        Align2::LEFT_CENTER,
        "palette.rs",
        FontId::monospace(11.0 * scale),
        ink(crate::exovoid::workbench("tab.activeForeground")),
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
        let y = inner.top() + (40.0 + 21.0 * i as f32) * scale;
        p.text(
            pos2(inner.left() + 14.0 * scale, y),
            Align2::LEFT_CENTER,
            (i + 1).to_string(),
            FontId::monospace(11.0 * scale),
            ink(crate::exovoid::workbench("editorLineNumber.foreground")),
        );
        let mut job = epaint::text::LayoutJob::default();
        for &(token, hex) in *spans {
            job.append(
                token,
                0.0,
                TextFormat::simple(FontId::monospace(13.0 * scale), ink(hex)),
            );
        }
        let line = p.layout_job(job);
        p.galley(
            pos2(inner.left() + 38.0 * scale, y - line.size().y / 2.0),
            line,
            Color32::WHITE,
        );
    }
}
