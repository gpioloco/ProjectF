//! Windows Terminal settings and a local Oh My Posh prompt.
use crate::{
    palette::readable,
    restore::{Component, RestorePoint},
    theme::{Theme, hex},
};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn export_prompt(point: &mut RestorePoint, root: &Path, t: &Theme) -> Result<PathBuf> {
    let path = root.join("prompt/wte.omp.json");
    let p = &t.palette;
    let prompt = serde_json::json!({
        "version": 3,
        "palette": {
            "pathBg": hex(p.accent),
            "pathFg": hex(readable(p.text, p.accent)),
            "git": hex(readable(p.secondary, p.background)),
            "prompt": hex(readable(p.text, p.background))
        },
        "blocks": [{
            "type": "prompt", "alignment": "left",
            "segments": [
                {"type":"path", "style":"plain", "foreground":"p:pathFg", "background":"p:pathBg", "template":" {{ .Path }} "},
                {"type":"git", "style":"plain", "foreground":"p:git", "template":" {{ .HEAD }} "},
                {"type":"text", "style":"plain", "foreground":"p:prompt", "template":" > "}
            ]
        }]
    });
    point.write(Component::Posh, &path, &serde_json::to_vec_pretty(&prompt)?)?;
    Ok(path)
}

#[cfg(any(windows, test))]
mod settings {
    use super::*;
    use crate::{
        config::{jsonc, object, set, unique},
        exovoid,
        palette::mix,
    };
    use anyhow::{Context, ensure};
    use jsonc_parser::cst::{CstInputValue, CstObject};
    const SCHEME: &str = "WTE Windows Theme Engine";

    fn scheme(profile: &CstObject, t: &Theme) -> Result<()> {
        let p = &t.palette;
        for (key, value) in [
            ("name", SCHEME.to_string()),
            ("background", hex(p.background)),
            ("foreground", hex(p.text)),
            ("cursorColor", hex(readable(p.accent, p.background))),
            ("selectionBackground", hex(mix(p.surface, p.accent, 0.5))),
        ] {
            set(profile, key, value)?;
        }
        for color in [
            "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
        ] {
            for prefix in ["", "Bright"] {
                let source = exovoid::workbench(&format!("terminal.ansi{prefix}{color}"));
                // Windows Terminal calls ANSI magenta "purple".
                let name = if color == "Magenta" { "Purple" } else { color };
                let key = if prefix.is_empty() {
                    name.to_ascii_lowercase()
                } else {
                    format!("bright{name}")
                };
                let rgb = if color == "Black" || color == "White" {
                    mix(exovoid::rgb(source), p.accent, 0.08)
                } else {
                    readable(
                        mix(exovoid::ink(t.mode, source), p.accent, 0.12),
                        p.background,
                    )
                };
                set(profile, &key, hex(rgb))?;
            }
        }
        Ok(())
    }

    fn profile(settings: &CstObject, t: &Theme, defaults: bool) -> Result<()> {
        if t.terminal {
            if defaults || settings.get("colorScheme").is_some() {
                set(settings, "colorScheme", SCHEME)?;
            }
            // These explicit overrides take precedence over a Terminal color scheme.
            for (key, color) in [
                ("background", t.palette.background),
                ("foreground", t.palette.text),
                (
                    "cursorColor",
                    readable(t.palette.accent, t.palette.background),
                ),
                (
                    "selectionBackground",
                    mix(t.palette.surface, t.palette.accent, 0.5),
                ),
            ] {
                if settings.get(key).is_some() {
                    set(settings, key, hex(color))?;
                }
            }
        }
        if t.terminal_fit {
            for (key, value) in [("padding", "0"), ("scrollbarState", "hidden")] {
                if defaults || settings.get(key).is_some() {
                    set(settings, key, value)?;
                }
            }
        }
        Ok(())
    }

    fn string_value(obj: &CstObject, key: &str) -> Option<String> {
        obj.get(key)?.value()?.as_string_lit()?.decoded_value().ok()
    }

    // Directly launched console apps can inherit profiles.defaults while the
    // initial window is sized for defaultProfile. Use the same font metrics for
    // inherited profiles, respecting any font choice already set in defaults.
    fn inherit_startup_font(
        obj: &CstObject,
        profiles: &CstObject,
        defaults: &CstObject,
    ) -> Result<()> {
        let Some(selected) = string_value(obj, "defaultProfile") else {
            return Ok(());
        };
        let Some(list) = profiles.array_value("list") else {
            return Ok(());
        };
        let font = list
            .elements()
            .into_iter()
            .filter_map(|n| n.as_object())
            .find(|p| {
                ["guid", "name"].iter().any(|key| {
                    string_value(p, key).is_some_and(|v| v.eq_ignore_ascii_case(&selected))
                })
            })
            .and_then(|p| p.object_value("font"));
        if let Some(font) = font {
            let inherited = object(defaults, "font")?;
            for key in ["face", "size", "weight", "cellWidth", "cellHeight"] {
                if inherited.get(key).is_some() {
                    continue;
                }
                if let Some(value) = font.get(key).and_then(|p| p.value()) {
                    if let Some(number) = value.as_number_lit() {
                        set(&inherited, key, CstInputValue::Number(number.to_string()))?;
                    } else if let Some(s) =
                        value.as_string_lit().and_then(|s| s.decoded_value().ok())
                    {
                        set(&inherited, key, s)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn configure(input: &str, t: &Theme) -> Result<String> {
        let (bom, root) = jsonc(input)?;
        let obj = root
            .object_value_or_create()
            .context("Terminal settings must be an object")?;
        if t.terminal {
            unique(&obj, "schemes")?;
            let schemes = obj
                .array_value_or_create("schemes")
                .context("Expected schemes to be a list")?;
            let mut current = None;
            for item in schemes.elements() {
                let item = item
                    .as_object()
                    .context("Expected a Terminal scheme object")?;
                unique(&item, "name")?;
                let name = string_value(&item, "name");
                if name.as_deref() == Some(SCHEME) {
                    ensure!(
                        current.is_none(),
                        "Duplicate WTE schemes in Terminal settings"
                    );
                    current = Some(item);
                }
            }
            let active = current.unwrap_or_else(|| {
                schemes
                    .append(CstInputValue::Object(vec![]))
                    .as_object()
                    .unwrap()
            });
            scheme(&active, t)?;
        }
        if t.terminal_fit {
            set(&obj, "snapToGridOnResize", false)?;
        }
        unique(&obj, "profiles")?;
        let legacy = obj.array_value("profiles");
        let old_format = legacy.is_some();
        let list = if old_format {
            legacy
        } else {
            let profiles = object(&obj, "profiles")?;
            let defaults = object(&profiles, "defaults")?;
            profile(&defaults, t, true)?;
            if t.terminal_fit {
                inherit_startup_font(&obj, &profiles, &defaults)?;
            }
            unique(&profiles, "list")?;
            let list = profiles.array_value("list");
            ensure!(
                list.is_some() || profiles.get("list").is_none(),
                "Expected profiles.list to be an array"
            );
            list
        };
        if let Some(list) = list {
            for item in list.elements() {
                let item = item
                    .as_object()
                    .context("Expected a Terminal profile object")?;
                profile(&item, t, old_format)?;
            }
        }
        Ok(format!("{bom}{root}"))
    }
}

#[cfg(any(windows, test))]
pub use settings::configure;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Appearance, Photo};
    use serde_json::{Value, json};
    #[test]
    fn new_terminal_profiles_inherit_fit_and_colors_independently() -> Result<()> {
        let original = json!({"defaultProfile":"{ABC}","snapToGridOnResize":true,"profiles":{
            "defaults":{"font":{"size":14},"colorScheme":"Campbell","padding":"8"},
            "list":[{"name":"PowerShell","guid":"{abc}","font":{"face":"Test Mono","size":18,"weight":"bold"},"padding":"8,8,100,8","scrollbarState":"always",
                     "colorScheme":{"dark":"Custom","light":"Day"},"commandline":"pwsh.exe"},
                    {"name":"WSL"}]},"schemes":[{"name":"Custom","red":"#dd0000"}]});
        let mut t = Theme::preset(Photo::BlueHour);
        let colored: Value = serde_json::from_str(&configure(&original.to_string(), &t)?)?;
        let profiles = &colored["profiles"];
        assert_eq!(colored["snapToGridOnResize"], true);
        assert_eq!(profiles["list"][0]["padding"], "8,8,100,8");
        assert_eq!(
            profiles["defaults"]["colorScheme"],
            profiles["list"][0]["colorScheme"]
        );
        assert_eq!(colored["schemes"][0], original["schemes"][0]);
        assert_eq!(colored["schemes"][1].as_object().unwrap().len(), 21);
        assert_eq!(
            colored["schemes"][1]["background"],
            hex(t.palette.background)
        );
        t.set_mode(Appearance::Light);
        let light: Value = serde_json::from_str(&configure(&colored.to_string(), &t)?)?;
        assert_eq!(light["schemes"].as_array().unwrap().len(), 2);
        assert_eq!(light["schemes"][1]["background"], hex(t.palette.background));

        t.terminal = false;
        t.terminal_fit = true;
        let fit: Value = serde_json::from_str(&configure(&original.to_string(), &t)?)?;
        assert_eq!(fit["snapToGridOnResize"], false);
        for entry in [&fit["profiles"]["defaults"], &fit["profiles"]["list"][0]] {
            assert_eq!(entry["padding"], "0");
            assert_eq!(entry["scrollbarState"], "hidden");
        }
        assert_eq!(fit["schemes"], original["schemes"]);
        assert_eq!(
            fit["profiles"]["list"][0]["colorScheme"],
            original["profiles"]["list"][0]["colorScheme"]
        );
        assert_eq!(fit["profiles"]["list"][1], original["profiles"]["list"][1]);
        assert_eq!(
            fit["profiles"]["defaults"]["font"],
            json!({"size":14,"face":"Test Mono","weight":"bold"})
        );
        assert!(
            colored["schemes"][1]["purple"].is_string()
                && colored["schemes"][1]["brightPurple"].is_string()
        );
        assert_eq!(fit["profiles"]["list"][0]["commandline"], "pwsh.exe");
        let old: Value = serde_json::from_str(&configure(r#"{"profiles":[{"name":"old"}]}"#, &t)?)?;
        assert_eq!(old["profiles"][0]["padding"], "0");
        assert!(configure(r#"{"profiles":{"list":false}}"#, &t).is_err());
        Ok(())
    }
}
