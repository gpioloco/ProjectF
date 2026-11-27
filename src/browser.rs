//! Local static themes. Each browser owns installation and activation.
use crate::{
    palette::readable,
    restore::{Component, RestorePoint},
    theme::{Theme, hex},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn chrome_folder(root: &Path) -> PathBuf {
    root.join("browsers").join("chrome")
}

// Chrome reads this generated pack before it reads the unpacked theme's colors.
// Removing it from our export makes Chrome rebuild it on its next full startup.
pub fn clear_chrome_cache(root: &Path) -> Result<()> {
    match std::fs::remove_file(chrome_folder(root).join("Cached Theme.pak")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result.context("Could not clear the Chrome theme cache in the WTE export. Fully exit Chrome and retry in WTE"),
    }
}

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
                "Load this shared folder once per Chrome profile using chrome://extensions → Developer mode → Load unpacked, if permitted. Future WTE themes with Chrome enabled update the same folder. Fully exit and reopen Chrome to see the latest palette."
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
        let icon = if t.mode == crate::theme::Appearance::Dark {
            crate::palette::with_contrast(p.accent, p.surface, 3.0)
        } else {
            readable(p.accent, p.surface)
        };
        let name = format!("WTE · {}", t.name);
        match self {
            Self::Chrome => json!({
                "manifest_version": 3, "name": name, "version": "1.0",
                "theme": { "colors": {
                    "frame": p.background, "frame_inactive": p.surface,
                    "toolbar": p.surface, "tab_text": tab_text, "tab_background_text": frame_text,
                    "toolbar_button_icon": icon, "bookmark_text": tab_text,
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
                        "icons": hex(icon), "icons_attention": hex(p.secondary),
                        "popup": hex(p.surface), "popup_text": hex(tab_text),
                        "ntp_background": hex(p.background), "ntp_text": hex(field_text)
                    }
                }
            }),
        }
    }

    pub fn export(self, point: &mut RestorePoint, root: &Path, theme: &Theme) -> Result<PathBuf> {
        ensure!(
            !theme.id.is_empty()
                && theme.id.len() <= 60
                && theme
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "Invalid browser theme identifier"
        );
        let json = serde_json::to_vec_pretty(&self.manifest(theme))?;
        match self {
            Self::Chrome => {
                let folder = chrome_folder(root);
                point.write(Component::Chrome, &folder.join("manifest.json"), &json)?;
                // Also retry cache removal when a previous apply wrote the manifest
                // successfully but Chrome still held its cache open.
                clear_chrome_cache(root)?;
                Ok(folder)
            }
            Self::Firefox => {
                let path = root
                    .join("browsers")
                    .join(format!("wte-{}", theme.id))
                    .join("firefox.xpi");
                point.write(
                    Component::Firefox,
                    &path,
                    &crate::archive(&[("manifest.json", &json)])?,
                )?;
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
    fn switching_chrome_themes_reuses_the_install_and_restores_the_original_export() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        let folder = chrome_folder(root);
        let manifest = folder.join("manifest.json");
        let cache = folder.join("Cached Theme.pak");
        let legacy = root.join("browsers/wte-canopy/chrome/manifest.json");
        fs::create_dir_all(legacy.parent().unwrap())?;
        fs::write(&legacy, b"legacy theme")?;
        fs::create_dir_all(&folder)?;
        fs::write(&manifest, b"original")?;
        let mut point = RestorePoint::load(root)?;
        let canopy = Theme::preset(Photo::Canopy);
        let sienna = Theme::preset(Photo::Sienna);
        assert_eq!(Browser::Chrome.export(&mut point, root, &canopy)?, folder);
        // Simulate a cache that cannot be removed, then retry the same palette.
        fs::create_dir(&cache)?;
        assert!(Browser::Chrome.export(&mut point, root, &sienna).is_err());
        fs::remove_dir(&cache)?;
        fs::write(&cache, b"Canopy cache")?;
        assert_eq!(Browser::Chrome.export(&mut point, root, &sienna)?, folder);
        let active: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
        assert_eq!(
            active["theme"]["colors"]["frame"],
            json!(sienna.palette.background)
        );
        assert!(!cache.exists());
        fs::write(&cache, b"Sienna cache")?;
        Browser::Firefox.export(&mut point, root, &canopy)?;
        assert!(cache.exists());
        assert!(
            apply::restore(root)?
                .entries
                .iter()
                .all(|entry| !entry.error)
        );
        assert_eq!(fs::read(manifest)?, b"original");
        assert_eq!(fs::read(legacy)?, b"legacy theme");
        assert!(!cache.exists());
        Ok(())
    }

    #[test]
    fn browser_only_apply_exports_selected_components_and_reuses_paths() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut t = Theme::preset(Photo::Canopy);
        t.desktop = false;
        t.vscode = false;
        t.nvim = false;
        t.glaze = false;
        t.terminal = false;
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
        assert!(
            Browser::Firefox
                .export(&mut RestorePoint::load(tmp.path())?, tmp.path(), &t)
                .is_err()
        );
        Ok(())
    }
}
