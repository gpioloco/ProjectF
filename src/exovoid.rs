//! ExoVoid Classic Syntax – Dark Soft Blue. The bundled VSIX contains this exact JSON.
use crate::theme::{Appearance, Rgb};
use serde_json::Value;
use std::sync::LazyLock;

pub const NAME: &str = "ExoVoid Classic Syntax - Dark Soft Blue";
pub const LIGHT_NAME: &str = "WTE ExoVoid Light";

pub fn name(mode: Appearance) -> &'static str {
    match mode {
        Appearance::Dark => NAME,
        Appearance::Light => LIGHT_NAME,
    }
}

/// Keep the source hue and use a darker ink on light editor surfaces.
pub fn ink(mode: Appearance, source: &str) -> Rgb {
    let rgb = rgb(source);
    if mode == Appearance::Dark {
        return rgb;
    }
    let [r, g, b] = rgb;
    let chroma = r.max(g).max(b) - r.min(g).min(b);
    let shade = if chroma < 12 { 0.70 } else { 0.28 };
    crate::palette::readable(crate::palette::mix(rgb, [0; 3], shade), [220; 3])
}

pub static SOURCE: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../assets/exovoid/ExoVoidClassicSyntax_DarkSoftBlue.json"
    ))
    .expect("bundled ExoVoid color theme")
});

pub fn workbench(key: &str) -> &'static str {
    SOURCE["colors"][key]
        .as_str()
        .expect("bundled workbench color")
}

pub fn rgb(hex: &str) -> Rgb {
    let value = u32::from_str_radix(&hex[1..7], 16).expect("bundled RGB color");
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

// Use the last matching exact TextMate scope, following the order of the upstream theme.
pub fn syntax(scope: &str) -> &'static str {
    SOURCE["tokenColors"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find_map(|rule| {
            let scopes = &rule["scope"];
            let matches = scopes.as_str() == Some(scope)
                || scopes
                    .as_array()
                    .is_some_and(|list| list.iter().any(|s| s.as_str() == Some(scope)));
            matches
                .then(|| rule["settings"]["foreground"].as_str())
                .flatten()
        })
        .unwrap_or_else(|| workbench("editor.foreground"))
}
