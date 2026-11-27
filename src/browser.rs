//! Local static themes. Each browser owns installation and activation.
use crate::{
    palette::readable,
    store::atomic_write,
    theme::{Theme, hex},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub enum Browser {
    Chrome,
    Firefox,
}
impl Browser {
    pub fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Firefox => "Firefox",
        }
    }

    pub fn instructions(self) -> &'static str {
        match self {
            Self::Chrome => {
                "Exported. In chrome://extensions, turn on Developer mode if permitted, choose Load unpacked and select this folder."
            }
            Self::Firefox => {
                "Exported. In about:debugging → This Firefox, choose Load Temporary Add-on and select this file. Firefox removes temporary themes on restart."
            }
        }
    }

    fn manifest(self, t: &Theme) -> Value {
        let p = &t.palette;
        let frame_text = readable(p.muted, p.background);
        let tab_text = readable(p.text, p.surface);
        let field_text = readable(p.text, p.background);
        let name = format!("WTE · {}", t.name);
        match self {
            Self::Chrome => json!({
                "manifest_version": 3, "name": name, "version": "1.0",
                "theme": { "colors": {
                    "frame": p.background, "frame_inactive": p.surface,
                    "toolbar": p.surface, "tab_text": tab_text, "tab_background_text": frame_text,
                    "toolbar_button_icon": readable(p.accent, p.surface), "bookmark_text": tab_text,
                    "omnibox_background": p.background, "omnibox_text": field_text,
                    "ntp_background": p.background, "ntp_text": field_text,
                    "ntp_link": readable(p.secondary, p.background)
                }}
            }),
            Self::Firefox => json!({
                "manifest_version": 2, "name": name, "version": "1.0",
                "browser_specific_settings": { "gecko": {
                    "id": format!("{}@themes.wte.invalid", t.id),
                    "data_collection_permissions": { "required": ["none"] }
                }},
                "theme": {
                    "properties": { "color_scheme": t.mode.label().to_ascii_lowercase() },
                    "colors": {
                        "frame": hex(p.background), "frame_inactive": hex(p.surface),
                        "tab_background_text": hex(frame_text), "tab_selected": hex(p.surface),
                        "tab_text": hex(tab_text), "tab_line": hex(p.accent),
                        "toolbar": hex(p.surface), "toolbar_text": hex(tab_text),
                        "toolbar_field": hex(p.background), "toolbar_field_text": hex(field_text),
                        "icons": hex(readable(p.accent, p.surface)), "icons_attention": hex(p.secondary),
                        "popup": hex(p.surface), "popup_text": hex(tab_text),
                        "ntp_background": hex(p.background), "ntp_text": hex(field_text)
                    }
                }
            }),
        }
    }

    pub fn export(self, root: &Path, theme: &Theme) -> Result<PathBuf> {
        ensure!(
            !theme.id.is_empty()
                && theme.id.len() <= 60
                && theme
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "Invalid browser theme identifier"
        );
        let directory = root.join("browsers").join(format!("wte-{}", theme.id));
        let json = serde_json::to_vec_pretty(&self.manifest(theme))?;
        match self {
            Self::Chrome => {
                let folder = directory.join("chrome");
                atomic_write(&folder.join("manifest.json"), &json)?;
                Ok(folder)
            }
            Self::Firefox => {
                let path = directory.join("firefox.xpi");
                atomic_write(&path, &crate::archive(&[("manifest.json", &json)])?)?;
                Ok(path)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        apply,
        theme::{Appearance, Photo},
    };
    use std::{fs, io::Read};

    #[test]
    fn browser_only_apply_exports_selected_components_and_reuses_paths() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut t = Theme::preset(Photo::Canopy);
        t.desktop = false;
        t.vscode = false;
        t.nvim = false;
        t.glaze = false;
        assert!(t.validate().is_err());
        t.chrome = true;
        t.name = r#"An "Alpine" morning"#.into();
        let report = apply::run(tmp.path(), &t)?;
        assert_eq!(report.entries.len(), 1);
        let chrome = report.entries[0]
            .path
            .as_ref()
            .unwrap()
            .join("manifest.json");
        let dark: Value = serde_json::from_slice(&fs::read(&chrome)?)?;
        assert_eq!(dark["manifest_version"], 3);
        assert_eq!(dark["theme"]["colors"]["toolbar"], json!(t.palette.surface));
        assert_eq!(dark["name"], format!("WTE · {}", t.name));

        t.set_mode(Appearance::Light);
        t.firefox = true;
        let report = apply::run(tmp.path(), &t)?;
        assert_eq!(report.entries.len(), 2);
        assert!(report.entries.iter().all(|r| !r.error));
        let light: Value = serde_json::from_slice(&fs::read(chrome)?)?;
        assert_eq!(
            light["theme"]["colors"]["ntp_background"],
            json!(t.palette.background)
        );
        assert_ne!(light["theme"], dark["theme"]);
        let xpi = report.entries[1].path.as_ref().unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(xpi)?)?;
        assert_eq!(zip.len(), 1);
        let mut bytes = Vec::new();
        zip.by_name("manifest.json")?.read_to_end(&mut bytes)?;
        let ff: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(ff["theme"]["properties"]["color_scheme"], "light");
        assert_eq!(ff["theme"]["colors"]["tab_line"], hex(t.palette.accent));
        assert_eq!(ff["theme"]["colors"]["toolbar"], hex(t.palette.surface));
        for manifest in [&dark, &ff] {
            assert!(manifest.get("permissions").is_none());
            assert!(manifest.get("background").is_none());
        }
        t.chrome = false;
        drop(zip);
        assert_eq!(apply::run(tmp.path(), &t)?.entries[0].component, "Firefox");
        t.id = "../escape".into();
        assert!(Browser::Firefox.export(tmp.path(), &t).is_err());
        Ok(())
    }
}
