use crate::theme::{Appearance, Theme};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

#[derive(Clone, Serialize, Deserialize)]
pub struct Library {
    pub themes: Vec<Theme>,
    #[serde(default = "default_selection")]
    pub selected: String,
    #[serde(default)]
    dark_accents_updated: bool,
    #[serde(default = "default_app_mode")]
    pub app_mode: Appearance,
}

fn default_selection() -> String {
    "canopy".into()
}

fn default_app_mode() -> Appearance {
    Appearance::Light
}
impl Default for Library {
    fn default() -> Self {
        Self {
            themes: Vec::new(),
            selected: default_selection(),
            dark_accents_updated: true,
            app_mode: default_app_mode(),
        }
    }
}

pub struct Store {
    pub root: PathBuf,
    _lock: fs::File,
}
impl Store {
    pub fn open() -> Result<Self> {
        let dirs =
            directories::BaseDirs::new().context("Find your local application data folder")?;
        let root = dirs.data_local_dir().join("WTE");
        fs::create_dir_all(&root)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("library.lock"))?;
        lock.try_lock()
            .context("WTE could not lock its library. Close any other WTE window and try again.")?;
        Ok(Self { root, _lock: lock })
    }
    pub fn load(&self) -> Result<Library> {
        let Some(data) = read_optional(&self.root.join("library.json"))? else {
            return Ok(Library::default());
        };
        let mut library: Library =
            serde_json::from_slice(&data).context("Read saved theme library")?;
        let mut ids = HashSet::new();
        for theme in &library.themes {
            theme.validate()?;
            ensure!(
                theme.id.starts_with("custom-") && ids.insert(&theme.id),
                "Invalid custom theme identifier."
            );
        }
        if !library.dark_accents_updated {
            for theme in &mut library.themes {
                theme.soften_dark_accents();
            }
            library.dark_accents_updated = true;
        }
        Ok(library)
    }
    pub fn save(&self, library: &Library) -> Result<()> {
        atomic_write(
            &self.root.join("library.json"),
            &serde_json::to_vec_pretty(library)?,
        )
    }
}

pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Read {}", path.display())),
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use crate::theme::Photo;

    #[test]
    fn darkens_legacy_accents_once_and_preserves_the_light_palette() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store {
            root: dir.path().into(),
            _lock: fs::File::create(dir.path().join("library.lock"))?,
        };
        let mut old = Theme::preset(Photo::BlueHour);
        old.id = "custom-blue".into();
        old.palette.accent = [144, 182, 246];
        old.palette.secondary = [187, 181, 228];
        let light = old.palette.variant(Appearance::Light);
        let mut old = serde_json::to_value(old)?;
        old.as_object_mut().unwrap().remove("alternate_palette");
        fs::write(
            dir.path().join("library.json"),
            serde_json::to_vec(&serde_json::json!({"themes": [old]}))?,
        )?;
        let mut library = store.load()?;
        let theme = &mut library.themes[0];
        assert_eq!(theme.palette, Theme::preset(Photo::BlueHour).palette);
        theme.set_mode(Appearance::Light);
        assert_eq!(theme.palette, light);
        theme.set_mode(Appearance::Dark);
        theme.palette.accent = [230, 120, 191];
        store.save(&library)?;
        assert_eq!(store.load()?.themes[0].palette.accent, [230, 120, 191]);
        Ok(())
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let resolved;
    let path = if path.is_symlink() {
        resolved = fs::canonicalize(path)
            .with_context(|| format!("Resolve linked file {}", path.display()))?;
        resolved.as_path()
    } else {
        path
    };
    let parent = path.parent().context("Missing parent directory")?;
    fs::create_dir_all(parent)?;
    let existing = match fs::metadata(path) {
        Ok(meta) => {
            ensure!(
                !meta.permissions().readonly(),
                "{} is read-only",
                path.display()
            );
            Some(meta)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    if let Some(meta) = existing {
        replace_existing(tmp, path, meta).with_context(|| format!("Replace {}", path.display()))?;
    } else {
        tmp.persist_noclobber(path)
            .with_context(|| format!("Create {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_existing(tmp: NamedTempFile, path: &Path, meta: fs::Metadata) -> Result<()> {
    tmp.as_file().set_permissions(meta.permissions())?;
    tmp.persist(path)?;
    Ok(())
}

#[cfg(windows)]
fn replace_existing(tmp: NamedTempFile, path: &Path, _meta: fs::Metadata) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr::null};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, ReplaceFileW, SetFileAttributesW,
    };
    let destination: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let tmp_path = tmp.into_temp_path();
    let temporary: Vec<u16> = tmp_path.as_os_str().encode_wide().chain([0]).collect();
    // Make the tempfile permanent and merge the original ACL and metadata.
    unsafe {
        if SetFileAttributesW(temporary.as_ptr(), FILE_ATTRIBUTE_NORMAL) == 0
            || ReplaceFileW(
                destination.as_ptr(),
                temporary.as_ptr(),
                null(),
                0,
                null(),
                null(),
            ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

pub fn asset_hash(data: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    hash.write(data);
    hash.finish()
}

pub fn stored_asset(root: &Path, folder: &str, extension: &str, data: &[u8]) -> Result<PathBuf> {
    let path = root
        .join(folder)
        .join(format!("{:016x}.{extension}", asset_hash(data)));
    if read_optional(&path)?.as_deref() != Some(data) {
        atomic_write(&path, data)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Photo;

    #[test]
    fn legacy_library_and_independent_appearance_preferences_round_trip() -> Result<()> {
        let mut original = Theme::preset(Photo::Sienna);
        original.id = "custom-legacy".into();
        let mut legacy = serde_json::to_value(&original)?;
        for key in [
            "mode",
            "alternate_palette",
            "chrome",
            "firefox",
            "posh",
            "terminal",
            "terminal_fit",
        ] {
            legacy.as_object_mut().unwrap().remove(key);
        }
        let dir = tempfile::tempdir()?;
        let store = Store {
            root: dir.path().into(),
            _lock: fs::File::create(dir.path().join("lock"))?,
        };
        atomic_write(
            &dir.path().join("library.json"),
            &serde_json::to_vec(&serde_json::json!({"themes":[legacy]}))?,
        )?;
        let mut library = store.load()?;
        assert_eq!(library.app_mode, Appearance::Light);
        assert_eq!(library.selected, "canopy");
        library.selected = original.id.clone();
        assert_eq!(library.themes[0].palette, original.palette);
        library.app_mode = Appearance::Dark;
        let theme = &mut library.themes[0];
        assert!(!theme.posh && !theme.terminal && !theme.terminal_fit);
        theme.set_mode(Appearance::Light);
        theme.palette.accent = [40, 80, 100];
        let custom_light = theme.palette.clone();
        store.save(&library)?;
        let mut loaded = store.load()?;
        assert_eq!(loaded.app_mode, Appearance::Dark);
        assert_eq!(loaded.selected, original.id);
        let saved = &mut loaded.themes[0];
        assert_eq!(saved.mode, Appearance::Light);
        saved.set_mode(Appearance::Dark);
        assert_eq!(saved.palette, original.palette);
        saved.set_mode(Appearance::Light);
        assert_eq!(saved.palette, custom_light);
        let replacement = Theme::preset(Photo::BlueHour).palette;
        saved.use_wallpaper_colors(replacement.clone());
        assert_eq!(saved.palette, replacement.variant(Appearance::Light));
        saved.set_mode(Appearance::Dark);
        assert_eq!(saved.palette, replacement);
        Ok(())
    }
}
