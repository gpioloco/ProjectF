//! Write-ahead restore points for the current user's personalization changes.
use crate::store::{atomic_write, read_optional};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Component {
    Desktop,
    Code,
    Neovim,
    Glaze,
    Chrome,
    Firefox,
}
impl Component {
    pub fn name(self) -> &'static str {
        match self {
            Self::Desktop => "Windows desktop",
            Self::Code => "Visual Studio Code",
            Self::Neovim => "Neovim",
            Self::Glaze => "GlazeWM",
            Self::Chrome => "Chrome",
            Self::Firefox => "Firefox",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Change {
    Directory {
        component: Component,
        path: PathBuf,
    },
    File {
        component: Component,
        path: PathBuf,
        original: Option<Vec<u8>>,
    },
    Desktop {
        theme: PathBuf,
    },
    Extension {
        mode: crate::theme::Appearance,
    },
}
impl Change {
    pub fn component(&self) -> Component {
        match self {
            Self::File { component, .. } | Self::Directory { component, .. } => *component,
            Self::Desktop { .. } => Component::Desktop,
            Self::Extension { .. } => Component::Code,
        }
    }
}

pub struct RestorePoint {
    path: PathBuf,
    pub changes: Vec<Change>,
}
impl RestorePoint {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("restore.json");
        let changes = read_optional(&path)?
            .map(|s| serde_json::from_slice(&s))
            .transpose()
            .context("Read the restore point")?
            .unwrap_or_default();
        Ok(Self { path, changes })
    }

    /// Earlier versions backed up config files but did not record their paths.
    /// Only the default VS Code profile had an unambiguous fixed destination.
    #[cfg(any(windows, test))]
    pub fn import_older_backups(&mut self, root: &Path, code: &Path, glaze: &Path) -> Result<()> {
        let marker = root.join("restore-history-v1");
        if marker.try_exists()? {
            return Ok(());
        }
        let mut older = Vec::new();
        if let Some(folders) = match fs::read_dir(root.join("backups")) {
            Ok(dirs) => Some(dirs),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        } {
            for item in folders {
                let item = item?;
                let name = item.file_name();
                if let Some((time, pid)) = name.to_string_lossy().split_once('-')
                    && let (Ok(time), Ok(_)) =
                        (u128::from_str_radix(time, 16), u32::from_str_radix(pid, 16))
                {
                    older.push((time, item.path()));
                }
            }
        }
        older.sort_by_key(|(time, _)| *time);
        for (component, name, target) in [
            (Component::Code, "vscode-settings.json", code),
            (Component::Glaze, "glazewm-config.yaml", glaze),
        ] {
            if !target.is_absolute()
                || self
                    .changes
                    .iter()
                    .any(|c| matches!(c, Change::File { path, .. } if path == target))
            {
                continue;
            }
            for (_, folder) in &older {
                let Some(original) = read_optional(&folder.join(name))? else {
                    continue;
                };
                let Some(current) = read_optional(target)? else {
                    break;
                };
                // GlazeWM destinations could be overridden. Match the earliest backup
                // only if all settings outside the WTE-managed borders identify it.
                if original != current
                    && (component == Component::Code || same_glaze_profile(&original, &current))
                {
                    self.remember(Change::File {
                        component,
                        path: target.into(),
                        original: Some(original),
                    })?;
                }
                break;
            }
        }
        atomic_write(&marker, b"1")
    }

    pub fn remember(&mut self, change: Change) -> Result<()> {
        self.changes.push(change);
        if let Err(e) = self.save() {
            self.changes.pop();
            return Err(e);
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        if self.changes.is_empty() {
            if let Err(e) = fs::remove_file(&self.path)
                && e.kind() != ErrorKind::NotFound
            {
                return Err(e.into());
            }
            Ok(())
        } else {
            atomic_write(&self.path, &serde_json::to_vec(&self.changes)?)
        }
    }

    pub fn write(&mut self, component: Component, path: &Path, bytes: &[u8]) -> Result<()> {
        let mut missing: Vec<_> = path
            .ancestors()
            .skip(1)
            .take_while(|p| !p.exists())
            .collect();
        missing.reverse();
        for dir in missing {
            if !self
                .changes
                .iter()
                .any(|c| matches!(c, Change::Directory { path, .. } if path == dir))
            {
                self.remember(Change::Directory {
                    component,
                    path: dir.into(),
                })?;
            }
        }
        let original = read_optional(path)?;
        if original.as_deref() == Some(bytes) {
            return Ok(());
        }
        if !self
            .changes
            .iter()
            .any(|c| matches!(c, Change::File { path: p, .. } if p == path))
        {
            self.remember(Change::File {
                component,
                path: path.into(),
                original,
            })?;
        }
        atomic_write(path, bytes)
    }

    pub fn finish(&mut self, index: usize) -> Result<()> {
        let change = self.changes.remove(index);
        if let Err(e) = self.save() {
            self.changes.insert(index, change);
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(any(windows, test))]
fn same_glaze_profile(first: &[u8], second: &[u8]) -> bool {
    let normalize = |bytes: &[u8]| -> Option<serde_yaml_ng::Value> {
        let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(bytes).ok()?;
        let root = value.as_mapping_mut()?;
        if let Some(effects) = root
            .get_mut("window_effects")
            .and_then(|v| v.as_mapping_mut())
        {
            for name in ["focused_window", "other_windows"] {
                if let Some(window) = effects.get_mut(name).and_then(|v| v.as_mapping_mut()) {
                    if let Some(border) = window.get_mut("border").and_then(|v| v.as_mapping_mut())
                    {
                        border.remove("enabled");
                        border.remove("color");
                        if border.is_empty() {
                            window.remove("border");
                        }
                    }
                    if window.is_empty() {
                        effects.remove(name);
                    }
                }
            }
            if effects.is_empty() {
                root.remove("window_effects");
            }
        }
        Some(value)
    };
    normalize(first).is_some_and(|v| Some(v) == normalize(second))
}

pub fn restore_file(
    path: &Path,
    original: Option<&[u8]>,
    backups: &Path,
    index: usize,
) -> Result<()> {
    let current = read_optional(path)?;
    if current.as_deref() == original {
        return Ok(());
    }
    if let Some(bytes) = &current {
        let name = path.file_name().context("Missing settings filename")?;
        atomic_write(&backups.join(index.to_string()).join(name), bytes)?;
    }
    match original {
        Some(bytes) => atomic_write(path, bytes),
        None => Ok(fs::remove_file(path)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_earliest_legacy_backups_once_and_checks_glaze_identity() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        let code = root.join("settings.json");
        let glaze = root.join("config.yaml");
        let initial_glaze = b"keybindings: []\n";
        let current_glaze = b"keybindings: []\nwindow_effects:\n  focused_window:\n    border:\n      enabled: true\n      color: '#112233'\n";
        for (apply, original) in [("2-a", "first"), ("10-b", "second")] {
            let backup = root.join("backups").join(apply);
            fs::create_dir_all(&backup)?;
            fs::write(backup.join("vscode-settings.json"), original)?;
            fs::write(backup.join("glazewm-config.yaml"), initial_glaze)?;
        }
        fs::write(&code, b"latest")?;
        fs::write(&glaze, current_glaze)?;
        assert!(!same_glaze_profile(
            b"keybindings: [different]",
            current_glaze
        ));
        let mut point = RestorePoint::load(root)?;
        point.import_older_backups(root, &code, &glaze)?;
        assert_eq!(point.changes.len(), 2);
        for (index, c) in point.changes.iter().enumerate() {
            let Change::File { path, original, .. } = c else {
                panic!("expected a settings file")
            };
            restore_file(
                path,
                original.as_deref(),
                &root.join("before-restore"),
                index,
            )?;
        }
        assert_eq!(fs::read(&code)?, b"first");
        assert_eq!(fs::read(&glaze)?, initial_glaze);
        point.finish(1)?;
        point.finish(0)?;
        fs::write(&code, b"new settings")?;
        point.import_older_backups(root, &code, &glaze)?;
        assert!(point.changes.is_empty());
        Ok(())
    }

    #[test]
    fn restore_uses_first_write_and_preserves_newer_edits_in_backups() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("WTE");
        let settings = dir.path().join("settings.json");
        let new_file = root.join("browsers/test/chrome/manifest.json");
        fs::write(&settings, b"original")?;
        {
            let mut point = RestorePoint::load(&root)?;
            point.write(Component::Code, &settings, b"theme one")?;
            point.write(Component::Chrome, &new_file, b"export")?;
        }
        let mut reopened = RestorePoint::load(&root)?;
        reopened.write(Component::Code, &settings, b"theme two")?;
        fs::write(&settings, b"later user edit")?;
        let report = crate::apply::restore(&root)?;
        assert!(report.entries.iter().all(|r| !r.error));
        assert_eq!(fs::read(&settings)?, b"original");
        assert!(!new_file.exists() && !root.join("browsers").exists());
        assert!(!root.join("restore.json").exists());
        let backup = fs::read_dir(root.join("backups"))?.next().unwrap()?.path();
        assert_eq!(
            fs::read(backup.join("0/settings.json"))?,
            b"later user edit"
        );
        Ok(())
    }

    #[test]
    fn failed_restore_can_be_retried_after_restart() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings.json");
        fs::write(&path, b"original")?;
        let mut point = RestorePoint::load(dir.path())?;
        point.write(Component::Code, &path, b"applied")?;
        fs::remove_file(&path)?;
        fs::create_dir(&path)?;
        assert!(
            crate::apply::restore(dir.path())?
                .entries
                .iter()
                .any(|r| r.error)
        );
        assert_eq!(RestorePoint::load(dir.path())?.changes.len(), 1);
        fs::remove_dir(&path)?;
        assert!(
            crate::apply::restore(dir.path())?
                .entries
                .iter()
                .all(|r| !r.error)
        );
        assert_eq!(fs::read(path)?, b"original");
        Ok(())
    }
}
