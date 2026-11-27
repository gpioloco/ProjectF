use crate::{
    browser::Browser,
    restore::{Change, Component, RestorePoint},
    theme::Theme,
};
#[cfg(any(windows, test))]
use crate::{
    config,
    store::{atomic_write, read_optional},
};
#[cfg(windows)]
use crate::{store::stored_asset, wallpaper};
use anyhow::Result;
#[cfg(any(windows, test))]
use anyhow::{Context, ensure};
use std::path::{Path, PathBuf};

/// Preserve the original bytes before replacing either application's existing settings.
#[cfg(any(windows, test))]
fn edit_settings(
    point: &mut RestorePoint,
    component: Component,
    path: &Path,
    backups: &Path,
    name: &str,
    edit: impl FnOnce(&str) -> Result<String>,
) -> Result<bool> {
    let original = read_optional(path)?;
    let input = std::str::from_utf8(original.as_deref().unwrap_or_default())
        .context("Expected UTF-8 settings")?;
    let output = edit(input)?;
    if Some(output.as_bytes()) == original.as_deref() {
        return Ok(false);
    }
    if let Some(bytes) = &original {
        atomic_write(&backups.join(name), bytes)?;
    }
    ensure!(
        read_optional(path)? == original,
        "Settings changed during apply. Retry with the latest file."
    );
    point.write(component, path, output.as_bytes())?;
    Ok(true)
}

#[cfg(any(windows, test))]
fn install_nvim(point: &mut RestorePoint, package: &Path, t: &Theme) -> Result<()> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let palette = config::nvim_palette(t);
    let mut hash = DefaultHasher::new();
    palette.hash(&mut hash);
    point.write(
        Component::Neovim,
        &package.join("colors/wte-palette.vim"),
        palette.as_bytes(),
    )?;
    point.write(
        Component::Neovim,
        &package.join("plugin/wte.vim"),
        config::NVIM_PLUGIN.as_bytes(),
    )?;
    let name = if t.vim_scheme.is_empty() {
        "wte-palette"
    } else {
        &t.vim_scheme
    };
    point.write(
        Component::Neovim,
        &package.join("active.txt"),
        format!("{name}\n{:016x}\n", hash.finish()).as_bytes(),
    )
}

#[cfg(any(windows, test))]
fn extension_id(mode: crate::theme::Appearance) -> &'static str {
    match mode {
        crate::theme::Appearance::Dark => "ginopioloco.exovoid",
        crate::theme::Appearance::Light => "wte-local.exovoid-light",
    }
}

#[cfg(any(windows, test))]
fn exovoid_registered(extensions: &Path, mode: crate::theme::Appearance) -> Result<bool> {
    let Some(index) = read_optional(&extensions.join("extensions.json"))? else {
        return Ok(false);
    };
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&index).context("Read VS Code’s extension list")?;
    let id = extension_id(mode);
    for entry in entries {
        if !entry["identifier"]["id"]
            .as_str()
            .is_some_and(|installed| installed.eq_ignore_ascii_case(id))
        {
            continue;
        }
        let Some(directory) = entry["relativeLocation"].as_str() else {
            continue;
        };
        if Path::new(directory).file_name().and_then(|s| s.to_str()) != Some(directory) {
            continue;
        }
        let manifest = extensions.join(directory).join("package.json");
        if let Some(bytes) = read_optional(&manifest)? {
            let package: serde_json::Value = serde_json::from_slice(&bytes)?;
            if package["contributes"]["themes"]
                .as_array()
                .is_some_and(|themes| {
                    themes
                        .iter()
                        .any(|theme| theme["label"] == crate::exovoid::name(mode))
                })
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[derive(Clone)]
pub struct Entry {
    pub component: &'static str,
    pub detail: String,
    pub error: bool,
    pub path: Option<PathBuf>,
}
#[derive(Default, Clone)]
pub struct Report {
    pub entries: Vec<Entry>,
}
impl Report {
    fn record(&mut self, component: &'static str, result: Result<String>) {
        let (detail, error) = match result {
            Ok(s) => (s, false),
            Err(e) => (format!("{e:#}"), true),
        };
        self.entries.push(Entry {
            component,
            detail,
            error,
            path: None,
        });
    }
    fn browser(&mut self, point: &mut RestorePoint, browser: Browser, root: &Path, theme: &Theme) {
        let (detail, path) = match browser.export(point, root, theme) {
            Ok(path) => (browser.instructions().into(), Some(path)),
            Err(error) => (format!("{error:#}"), None),
        };
        self.entries.push(Entry {
            component: browser.name(),
            detail,
            error: path.is_none(),
            path,
        });
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        env,
        ffi::OsStr,
        os::windows::{ffi::OsStrExt, process::CommandExt},
        process::{Command, Stdio},
    };
    use windows_sys::Win32::{
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        },
        UI::{
            Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    pub fn desktop(point: &mut RestorePoint, root: &Path, t: &Theme) -> Result<String> {
        let image = wallpaper::render(&wallpaper::load(&t.wallpaper)?, t.effects);
        let photo = stored_asset(root, "renders", "jpg", &wallpaper::jpeg(&image)?)?;
        let contents =
            config::desktop_theme(t, photo.to_str().context("Invalid wallpaper filename")?)?;
        let base = directories::BaseDirs::new().context("Find the Windows user theme folder")?;
        let hash = crate::store::asset_hash(&contents);
        let file = base
            .data_local_dir()
            .join("Microsoft/Windows/Themes")
            .join(format!("WTE-{hash:016x}.theme"));
        if !point
            .changes
            .iter()
            .any(|c| matches!(c, Change::Desktop { .. }))
        {
            let theme = crate::desktop::snapshot(root)?;
            point.remember(Change::Desktop { theme })?;
        }
        point.write(Component::Desktop, &file, &contents)?;
        crate::desktop::activate(&file, &photo, &t.name, open_theme)
    }

    pub fn open_theme(file: &Path) -> Result<()> {
        let path: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();
        let verb: Vec<u16> = OsStr::new("open").encode_wide().chain([0]).collect();
        // The shell handler can use COM. Finish the handoff before this worker exits.
        let mut request = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            lpFile: path.as_ptr(),
            lpVerb: verb.as_ptr(),
            nShow: SW_SHOWNORMAL,
            ..Default::default()
        };
        unsafe {
            let hr = CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            );
            ensure!(
                hr >= 0,
                "Could not initialize the Windows shell (HRESULT {hr:#x})."
            );
            let result = ShellExecuteExW(&mut request);
            let error = (result == 0).then(std::io::Error::last_os_error);
            CoUninitialize();
            if let Some(e) = error {
                return Err(e).context("Open the theme in Windows Personalization");
            }
        }
        Ok(())
    }

    fn ensure_exovoid(
        point: &mut RestorePoint,
        root: &Path,
        base: &directories::BaseDirs,
        mode: crate::theme::Appearance,
    ) -> Result<bool> {
        let extensions = base.home_dir().join(".vscode/extensions");
        if exovoid_registered(&extensions, mode)? {
            return Ok(false);
        }

        let light;
        let (package, bytes): (&str, &[u8]) = match mode {
            crate::theme::Appearance::Dark => (
                "exovoid-1.4.0.vsix",
                include_bytes!("../assets/exovoid/exovoid-1.4.0.vsix"),
            ),
            crate::theme::Appearance::Light => {
                light = config::light_vsix()?;
                ("wte-exovoid-light-1.0.0.vsix", &light)
            }
        };
        let folder = root.join("extensions");
        let archive = folder.join(package);
        if read_optional(&archive)?.as_deref() != Some(bytes) {
            atomic_write(&archive, bytes)?;
        }
        let help = format!(
            "In VS Code, use ‘Extensions: Install from VSIX…’ with {} and apply the theme again.",
            archive.display()
        );
        let cli = code_cli(base).with_context(|| help.clone())?;
        // Leave previously installed versions under VS Code's extension manager.
        if extension_present(&extensions, mode)? {
            anyhow::bail!("The installed extension does not provide the requested base. {help}");
        }
        let already_tracked = point
            .changes
            .iter()
            .any(|c| matches!(c, Change::Extension { mode: m } if *m == mode));
        if !already_tracked {
            point.remember(Change::Extension { mode })?;
        }
        // Both arguments are fixed. Let VS Code validate the local package and enforce its policies.
        let result = Command::new(cli)
            .current_dir(folder)
            .args(["--install-extension", package])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .output()
            .context("Start the VS Code extension manager")
            .and_then(|out| {
                ensure!(
                    out.status.success(),
                    "VS Code could not install ExoVoid. {} {}",
                    String::from_utf8_lossy(&out.stderr).trim(),
                    help
                );
                Ok(())
            });
        if result.is_err() && !already_tracked && !extension_present(&extensions, mode)? {
            point.finish(point.changes.len() - 1)?;
        }
        result?;
        Ok(true)
    }

    fn code_cli(base: &directories::BaseDirs) -> Result<PathBuf> {
        let user_bin = base.data_local_dir().join("Programs/Microsoft VS Code/bin");
        let system_bin =
            env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Microsoft VS Code/bin"));
        let path = env::var_os("PATH").unwrap_or_default();
        std::iter::once(user_bin)
            .chain(system_bin)
            .chain(env::split_paths(&path))
            .filter(|dir| dir.is_absolute() && dir.join("../Code.exe").is_file())
            .map(|dir| dir.join("code.cmd"))
            .find(|file| file.is_file())
            .context("Could not find the VS Code CLI")
    }

    fn extension_present(dir: &Path, mode: crate::theme::Appearance) -> Result<bool> {
        let Some(bytes) = read_optional(&dir.join("extensions.json"))? else {
            return Ok(false);
        };
        let entries: Vec<serde_json::Value> = serde_json::from_slice(&bytes)?;
        Ok(entries.iter().any(|e| {
            e["identifier"]["id"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(extension_id(mode)))
        }))
    }

    pub fn remove_extension(mode: crate::theme::Appearance) -> Result<()> {
        let base = directories::BaseDirs::new().context("Locate VS Code")?;
        if extension_present(&base.home_dir().join(".vscode/extensions"), mode)? {
            let status = Command::new(code_cli(&base)?)
                .args(["--uninstall-extension", extension_id(mode)])
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .output()?;
            ensure!(
                status.status.success(),
                "VS Code could not remove {}: {}",
                extension_id(mode),
                String::from_utf8_lossy(&status.stderr).trim()
            );
        }
        Ok(())
    }
    pub fn vscode(
        point: &mut RestorePoint,
        t: &Theme,
        root: &Path,
        backups: &Path,
    ) -> Result<String> {
        let base = directories::BaseDirs::new().context("Find roaming application data")?;
        let user = base.data_dir().join("Code/User");
        if !user.is_dir() {
            return Ok("Skipped. The default VS Code user profile was not found.".into());
        }
        let path = user.join("settings.json");
        let mut installed = false;
        // Validate the user's JSONC before installing an extension.
        if t.code_theme.trim().is_empty() {
            let current = read_optional(&path)?.unwrap_or_default();
            config::vscode(std::str::from_utf8(&current)?, t)?;
            installed = ensure_exovoid(point, root, &base, t.mode)?;
        }
        edit_settings(
            point,
            Component::Code,
            &path,
            backups,
            "vscode-settings.json",
            |s| config::vscode(s, t),
        )?;
        Ok(if installed {
            "Installed the bundled ExoVoid theme and updated the default VS Code profile."
        } else {
            "Updated the default VS Code profile."
        }
        .into())
    }

    pub fn nvim(point: &mut RestorePoint, t: &Theme) -> Result<String> {
        let Some(profile) = crate::nvim::locate()? else {
            return Ok(
                "Skipped. No Neovim executable or matching profile in Local or Roaming AppData."
                    .into(),
            );
        };
        let package = profile.package();
        install_nvim(point, &package, t)?;
        point.write(
            Component::Neovim,
            &profile.loader(),
            config::NVIM_CONFIG_LOADER.as_bytes(),
        )?;
        Ok(format!(
            "Installed in {} with the startup hook in {}. Restart Neovim after first install, then refocus to update.",
            package.display(),
            profile.config.display()
        ))
    }

    pub fn glaze(point: &mut RestorePoint, t: &Theme, backups: &Path) -> Result<String> {
        let base = directories::BaseDirs::new().context("Find the user profile")?;
        let path = env::var_os("GLAZEWM_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".glzr/glazewm/config.yaml"));
        if !path.try_exists()? {
            return Ok("Skipped. GlazeWM configuration was not found.".into());
        }
        ensure!(path.is_absolute(), "GLAZEWM_CONFIG_PATH must be absolute.");
        let changed = edit_settings(
            point,
            Component::Glaze,
            &path,
            backups,
            "glazewm-config.yaml",
            |s| config::glazewm(s, t),
        )?;
        if !changed {
            return Ok("Border colors already match.".into());
        }
        Ok(reload_glaze())
    }

    pub fn reload_glaze() -> String {
        let dirs = env::var_os("ProgramFiles").into_iter().map(PathBuf::from);
        let path = env::var_os("PATH").unwrap_or_default();
        let cli = dirs
            .flat_map(|p| [p.join("glzr.io/GlazeWM/cli"), p.join("glzr.io/cli")])
            .chain(env::split_paths(&path))
            .filter(|d| d.is_absolute())
            .map(|d| d.join("glazewm.exe"))
            .find(|p| p.is_file());
        let run = |command| {
            cli.as_ref().is_some_and(|exe| {
                Command::new(exe)
                    .args(["command", command])
                    .creation_flags(0x08000000)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            })
        };
        if !run("wm-reload-config") {
            return "Saved the configuration. Reload GlazeWM to use it.".into();
        }
        if !run("wm-redraw") {
            return "Reloaded GlazeWM. Use its redraw binding if a tile needs resizing.".into();
        }
        "Reloaded the configuration and redrew the tiled windows.".into()
    }

    pub fn terminal(report: &mut Report, point: &mut RestorePoint, t: &Theme, backups: &Path) {
        let Some(base) = directories::BaseDirs::new() else {
            report.record(
                "Windows Terminal",
                Err(anyhow::anyhow!("Could not locate Local AppData")),
            );
            return;
        };
        terminal_settings(report, point, t, backups, base.data_local_dir());
    }
}

#[cfg(any(windows, test))]
fn terminal_settings(
    report: &mut Report,
    point: &mut RestorePoint,
    t: &Theme,
    backups: &Path,
    local: &Path,
) {
    let mut found = false;
    for (label, relative, backup) in [
        (
            "Windows Terminal",
            "Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
            "terminal-stable.json",
        ),
        (
            "Windows Terminal Preview",
            "Packages/Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe/LocalState/settings.json",
            "terminal-preview.json",
        ),
        (
            "Windows Terminal Canary",
            "Packages/Microsoft.WindowsTerminalCanary_8wekyb3d8bbwe/LocalState/settings.json",
            "terminal-canary.json",
        ),
        (
            "Windows Terminal (unpackaged)",
            "Microsoft/Windows Terminal/settings.json",
            "terminal-unpackaged.json",
        ),
    ] {
        let path = local.join(relative);
        match path.try_exists() {
            Ok(false) => continue,
            Err(e) => {
                found = true;
                report.record(label, Err(e.into()));
                continue;
            }
            Ok(true) => found = true,
        }
        let result = edit_settings(point, Component::Terminal, &path, backups, backup, |s| {
            crate::terminal::configure(s, t)
        });
        report.record(
            label,
            result.map(|changed| {
                if changed {
                    "Updated Terminal settings. Open a fresh tab to pick up profile changes.".into()
                } else {
                    "Terminal settings already match.".into()
                }
            }),
        );
    }
    if !found {
        report.record(
            "Windows Terminal",
            Ok("No settings file found. Open Terminal, save a setting and apply again.".into()),
        );
    }
}

#[cfg(windows)]
fn desktop_components(point: &mut RestorePoint, report: &mut Report, root: &Path, t: &Theme) {
    // Files from the same apply share a backup directory. Create it only on the first actual edit.
    let backups = root.join("backups").join(crate::unique_id());
    if t.desktop {
        report.record("Windows desktop", platform::desktop(point, root, t));
    }
    if t.vscode {
        report.record(
            "Visual Studio Code",
            platform::vscode(point, t, root, &backups),
        );
    }
    if t.nvim {
        report.record("Neovim", platform::nvim(point, t));
    }
    if t.terminal || t.terminal_fit {
        platform::terminal(report, point, t, &backups);
    }
    if t.glaze {
        report.record("GlazeWM", platform::glaze(point, t, &backups));
    }
    if backups.is_dir() {
        report.record(
            "Original settings",
            Ok(format!("Backups saved in {}", backups.display())),
        );
    }
}

fn restore_point(root: &Path) -> Result<RestorePoint> {
    let point = RestorePoint::load(root)?;
    #[cfg(windows)]
    {
        let base = directories::BaseDirs::new().context("Locate application settings")?;
        let mut point = point;
        let glaze = std::env::var_os("GLAZEWM_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".glzr/glazewm/config.yaml"));
        point.import_older_backups(
            root,
            &base.data_dir().join("Code/User/settings.json"),
            &glaze,
        )?;
        Ok(point)
    }
    #[cfg(not(windows))]
    Ok(point)
}

pub fn run(root: &Path, t: &Theme) -> Result<Report> {
    t.validate()?;
    let mut point = restore_point(root)?;
    let mut report = Report::default();
    #[cfg(windows)]
    desktop_components(&mut point, &mut report, root, t);
    #[cfg(not(windows))]
    if t.desktop || t.vscode || t.nvim || t.glaze || t.terminal || t.terminal_fit {
        report.entries.push(Entry {
            component: "Desktop integrations",
            detail: "These components require Windows.".into(),
            error: true,
            path: None,
        });
    }
    if t.posh {
        match crate::terminal::export_prompt(&mut point, root, t) {
            Ok(path) => report.entries.push(Entry {
                component: Component::Posh.name(), error: false, path: Some(path),
                detail: "Use this path as --config in your Oh My Posh initialization once. Reopen the shell after switching themes.".into(),
            }),
            Err(e) => report.record(Component::Posh.name(), Err(e)),
        }
    }
    if t.chrome {
        report.browser(&mut point, Browser::Chrome, root, t);
    }
    if t.firefox {
        report.browser(&mut point, Browser::Firefox, root, t);
    }
    Ok(report)
}

pub fn restore(root: &Path) -> Result<Report> {
    let mut point = restore_point(root)?;
    let mut report = Report::default();
    if point.changes.is_empty() {
        report.record(
            "Restore",
            Ok(
                "No saved changes to restore. The next theme you apply starts a new restore point."
                    .into(),
            ),
        );
        return Ok(report);
    }
    let backups = root
        .join("backups")
        .join(format!("restore-{}", crate::unique_id()));
    let mut restored = Vec::new();
    // Switch away from the installed theme before deleting its file. On failure
    // keep the desktop entries for a retry while restoring the other components.
    #[cfg(windows)]
    if let Some((i, theme)) = point.changes.iter().enumerate().find_map(|(i, c)| match c {
        Change::Desktop { theme } => Some((i, theme)),
        _ => None,
    }) {
        match crate::desktop::restore(theme, platform::open_theme) {
            Ok(()) => {
                point.finish(i)?;
                restored.push(Component::Desktop);
            }
            Err(e) => report.record(Component::Desktop.name(), Err(e)),
        }
    }
    for i in (0..point.changes.len()).rev() {
        let change = &point.changes[i];
        let component = change.component();
        #[cfg(windows)]
        if component == Component::Desktop
            && report
                .entries
                .iter()
                .any(|e| e.component == component.name() && e.error)
        {
            continue;
        }
        let result = match change {
            Change::Directory { path, .. } if point.changes[i+1..].iter().any(|c|
                matches!(c, Change::File { path: child, .. } | Change::Directory { path: child, .. } if child.starts_with(path))) => continue,
            Change::Directory { path, .. } => match std::fs::remove_dir(path) {
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    ) =>
                {
                    Ok(())
                }
                result => result.map_err(Into::into),
            },
            Change::File { path, original, .. } => {
                crate::restore::restore_file(path, original.as_deref(), &backups, i)
                    .map_err(|e| anyhow::anyhow!("{}: {e:#}", path.display()))
            }
            #[cfg(windows)]
            Change::Desktop { .. } => continue,
            #[cfg(windows)]
            Change::Extension { mode } => platform::remove_extension(*mode),
            #[cfg(not(windows))]
            _ => Err(anyhow::anyhow!("Restore this component on Windows.")),
        };
        match result {
            Ok(()) => {
                point.finish(i)?;
                if !restored.contains(&component) {
                    restored.push(component);
                }
            }
            Err(e) => report.record(component.name(), Err(e)),
        }
    }
    for component in restored {
        if report
            .entries
            .iter()
            .any(|e| e.error && e.component == component.name())
        {
            continue;
        }
        let detail = match component {
            Component::Desktop => "Restored the previous desktop theme.",
            Component::Code => "Restored your previous VS Code setup. Reload the editor window.",
            Component::Neovim => {
                "Restored the previous files. Restart Neovim to load your original configuration."
            }
            #[cfg(windows)]
            Component::Glaze => {
                report.record(component.name(), Ok(platform::reload_glaze()));
                continue;
            }
            #[cfg(not(windows))]
            Component::Glaze => "Restored the original configuration. Reload GlazeWM.",
            Component::Terminal => "Restored your Windows Terminal settings. Open a fresh tab.",
            Component::Posh => {
                "Restored the previous prompt file. Reopen the shell. If your profile points directly to the removed WTE file, reselect your previous Oh My Posh config."
            }
            Component::Chrome => {
                "Restored the export files. If you loaded a WTE theme, select your previous theme in Chrome’s Appearance settings."
            }
            Component::Firefox => {
                "Restored the export files. If you enabled a WTE theme, reselect your previous theme in Firefox’s Add-ons Manager."
            }
        };
        report.record(component.name(), Ok(detail.into()));
    }
    if backups.exists() {
        report.record(
            "Before restore",
            Ok(format!(
                "Replaced files were backed up in {}",
                backups.display()
            )),
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Photo;
    use std::fs;

    #[test]
    fn terminal_and_prompt_restore_together_after_multiple_themes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("WTE");
        let local = dir.path().join("Local");
        let path = local.join("Microsoft/Windows Terminal/settings.json");
        let preview = local.join(
            "Packages/Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe/LocalState/settings.json",
        );
        let original = b"{\r\n // preserve this comment\r\n \"initialCols\": 220, \"profiles\": {\"defaults\": {\"padding\": \"8,8,100,8\"}},\r\n}";
        atomic_write(&path, original)?;
        atomic_write(&preview, b"{oops")?;
        let mut point = RestorePoint::load(&root)?;
        let mut prompt = PathBuf::new();
        for photo in [Photo::Sienna, Photo::BlueHour] {
            let mut report = Report::default();
            let mut theme = Theme::preset(photo);
            theme.terminal_fit = true;
            terminal_settings(
                &mut report,
                &mut point,
                &theme,
                &root.join("backups/test"),
                &local,
            );
            assert_eq!(report.entries.len(), 2);
            assert!(
                report
                    .entries
                    .iter()
                    .any(|e| e.error && e.component == "Windows Terminal Preview")
            );
            prompt = crate::terminal::export_prompt(&mut point, &root, &theme)?;
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&fs::read(&prompt)?)?["palette"]["pathBg"],
                crate::theme::hex(theme.palette.accent)
            );
            assert!(fs::read_to_string(&path)?.contains("// preserve this comment"));
        }
        assert!(restore(&root)?.entries.iter().all(|e| !e.error));
        assert_eq!(fs::read(&path)?, original);
        assert_eq!(fs::read(preview)?, b"{oops");
        assert!(!prompt.exists());
        Ok(())
    }

    #[test]
    fn vscode_updates_owned_keys_and_preserves_jsonc_on_repeat() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings.json");
        let backup = dir.path().join("backup");
        let original = "\u{feff}{\r\n  // Font preference\r\n  \"editor.fontSize\": 18,\r\n  \"workbench.colorCustomizations\": {\"[Other]\": {\"focusBorder\": \"#abcdef\"}},\r\n}\r\n";
        fs::write(&path, original)?;
        for photo in Photo::ALL {
            let theme = Theme::preset(photo);
            edit_settings(
                &mut RestorePoint::load(dir.path())?,
                Component::Code,
                &path,
                &backup,
                "vscode.json",
                |s| config::vscode(s, &theme),
            )?;
            let result = fs::read_to_string(&path)?;
            assert!(result.starts_with('\u{feff}') && result.contains("// Font preference"));
            assert!(result.contains("\"editor.fontSize\": 18") && result.contains("#abcdef"));
            assert!(result.contains(&crate::theme::hex(theme.palette.accent)));
            let repeat = config::vscode(&result, &theme)?;
            assert_eq!(repeat, result);
            if photo == Photo::Canopy {
                assert_eq!(fs::read_to_string(backup.join("vscode.json"))?, original);
            }
        }
        Ok(())
    }

    #[test]
    fn generated_workbench_inherits_all_exovoid_font_and_syntax_colors() -> Result<()> {
        use crate::exovoid;
        assert_eq!(exovoid::SOURCE["name"], exovoid::NAME);
        for photo in Photo::ALL {
            let theme = Theme::preset(photo);
            let result: serde_json::Value = serde_json::from_str(&config::vscode("{}", &theme)?)?;
            assert_eq!(result["workbench.colorTheme"], exovoid::NAME);
            assert!(result.get("editor.tokenColorCustomizations").is_none());
            assert!(
                result
                    .get("editor.semanticTokenColorCustomizations")
                    .is_none()
            );
            let changes = &result["workbench.colorCustomizations"][format!("[{}]", exovoid::NAME)];
            for (key, original) in exovoid::SOURCE["colors"].as_object().unwrap() {
                let id = key.to_ascii_lowercase();
                if id.contains("foreground") || id.starts_with("terminal.ansi") {
                    assert!(changes.get(key).is_none(), "{key}");
                } else {
                    let value = changes[key].as_str().unwrap();
                    assert_eq!(&value[7..], &original.as_str().unwrap()[7..], "{key}");
                }
            }
            assert_eq!(
                changes["editor.background"],
                crate::theme::hex(theme.palette.background)
            );
            assert_eq!(
                changes["sideBar.background"],
                crate::theme::hex(theme.palette.surface)
            );
            assert_eq!(
                changes["focusBorder"],
                crate::theme::hex(theme.palette.accent)
            );
        }
        Ok(())
    }

    #[test]
    fn light_editors_use_the_same_ink_and_switch_back_to_the_dark_base() -> Result<()> {
        use crate::{
            exovoid,
            theme::{Appearance, hex},
        };
        use std::io::Cursor;
        let mut t = Theme::preset(Photo::BlueHour);
        t.set_mode(Appearance::Light);
        let mut vsix = zip::ZipArchive::new(Cursor::new(config::light_vsix()?))?;
        let package: serde_json::Value =
            serde_json::from_reader(vsix.by_name("extension/package.json")?)?;
        assert_eq!(package["contributes"]["themes"][0]["uiTheme"], "vs");
        let base: serde_json::Value =
            serde_json::from_reader(vsix.by_name("extension/theme.json")?)?;
        assert_eq!(base["name"], exovoid::LIGHT_NAME);
        let light = config::vscode("{}", &t)?;
        let settings: serde_json::Value = serde_json::from_str(&light)?;
        let scope = format!("[{}]", exovoid::LIGHT_NAME);
        assert_eq!(settings["workbench.colorTheme"], exovoid::LIGHT_NAME);
        assert_eq!(
            settings["workbench.colorCustomizations"][&scope]["editor.background"],
            hex(t.palette.background)
        );
        let vim = config::nvim_palette(&t);
        assert!(vim.contains("set background=light"));
        for (rule, original) in base["tokenColors"]
            .as_array()
            .unwrap()
            .iter()
            .zip(exovoid::SOURCE["tokenColors"].as_array().unwrap())
        {
            assert_eq!(rule["scope"], original["scope"]);
            assert_eq!(
                rule["settings"]["fontStyle"],
                original["settings"]["fontStyle"]
            );
            if let Some(fg) = original["settings"]["foreground"].as_str() {
                assert_eq!(
                    rule["settings"]["foreground"],
                    hex(exovoid::ink(t.mode, fg))
                );
            }
        }
        let fg = base["semanticTokenColors"]["numberLiteral"]
            .as_str()
            .unwrap();
        assert!(vim.contains(&format!("@lsp.type.numberLiteral guifg={fg} ")));
        let ini: String = String::from_utf16(
            &config::desktop_theme(&t, "local.jpg")?[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect::<Vec<_>>(),
        )?;
        assert!(ini.contains("SystemMode=Light\r\nAppMode=Light\r\n"));
        t.set_mode(Appearance::Dark);
        let dark: serde_json::Value = serde_json::from_str(&config::vscode(&light, &t)?)?;
        assert_eq!(dark["workbench.colorTheme"], exovoid::NAME);
        assert_eq!(
            dark["workbench.colorCustomizations"][&scope],
            settings["workbench.colorCustomizations"][&scope]
        );
        Ok(())
    }

    #[test]
    fn recognizes_installed_exovoid_only_when_it_contributes_the_requested_theme() -> Result<()> {
        let dir = tempfile::tempdir()?;
        assert!(!exovoid_registered(
            dir.path(),
            crate::theme::Appearance::Dark
        )?);
        let extension = dir.path().join("ginopioloco.exovoid-1.4.0");
        fs::create_dir(&extension)?;
        fs::write(
            dir.path().join("extensions.json"),
            r#"[{"identifier":{"id":"GinoPioLoco.ExoVoid"},"relativeLocation":"ginopioloco.exovoid-1.4.0"}]"#,
        )?;
        let manifest = extension.join("package.json");
        fs::write(
            &manifest,
            r#"{"contributes":{"themes":[{"label":"ExoVoid"}]}}"#,
        )?;
        assert!(!exovoid_registered(
            dir.path(),
            crate::theme::Appearance::Dark
        )?);
        fs::write(
            manifest,
            serde_json::to_vec(
                &serde_json::json!({"contributes":{"themes":[{"label":crate::exovoid::NAME}]}}),
            )?,
        )?;
        assert!(exovoid_registered(
            dir.path(),
            crate::theme::Appearance::Dark
        )?);
        Ok(())
    }

    #[test]
    fn malformed_foreign_settings_remain_intact() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings.json");
        let theme = Theme::preset(Photo::Sienna);
        for text in [
            "{\"editor.fontSize\": 12",
            "[]",
            "{\"a\":1 \"b\":2}",
            "{\"workbench.colorCustomizations\":false}",
            "{\"workbench.colorTheme\":1,\"workbench.colorTheme\":2}",
        ] {
            fs::write(&path, text)?;
            assert!(
                edit_settings(
                    &mut RestorePoint::load(dir.path())?,
                    Component::Code,
                    &path,
                    dir.path(),
                    "backup",
                    |s| config::vscode(s, &theme)
                )
                .is_err()
            );
            assert_eq!(fs::read_to_string(&path)?, text);
            assert!(!dir.path().join("backup").exists());
        }
        Ok(())
    }

    #[test]
    fn glazewm_inserts_missing_maps_and_preserves_rules_and_comments() -> Result<()> {
        let t = Theme::preset(Photo::Canopy);
        for input in [
            "# custom setup\nkeybindings:\n  - commands: ['wm-reload-config']\n    bindings: ['alt+r']\n",
            "general: {focus_follows_cursor: true}\nwindow_effects:\n  focused_window:\n    # keep this border note\n    border:\n      enabled: false\n      color: '#ffffff'\n    transparency: {enabled: false}\n",
        ] {
            let output = config::glazewm(input, &t)?;
            let settings: serde_yaml_ng::Value = serde_yaml_ng::from_str(&output)?;
            assert_eq!(
                settings["window_effects"]["focused_window"]["border"]["color"],
                crate::theme::hex(t.palette.accent)
            );
            assert_eq!(
                settings["window_effects"]["other_windows"]["border"]["color"],
                crate::theme::hex(t.palette.surface)
            );
            let original: serde_yaml_ng::Value = serde_yaml_ng::from_str(input)?;
            for key in ["focused_window", "other_windows"] {
                assert_eq!(
                    settings["window_effects"][key]["border"]["enabled"],
                    original["window_effects"][key]["border"]["enabled"]
                );
            }
            for line in input.lines().filter(|l| l.trim_start().starts_with('#')) {
                assert!(output.contains(line));
            }
            assert_eq!(config::glazewm(&output, &t)?, output);
        }
        assert!(config::glazewm("window_effects: false", &t).is_err());
        Ok(())
    }

    #[test]
    fn installed_editor_override_and_nvim_activation() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut theme = Theme::preset(Photo::BlueHour);
        let package = dir.path().join("site/pack/wte/start/wte");
        install_nvim(&mut RestorePoint::load(dir.path())?, &package, &theme)?;
        assert!(
            fs::read_to_string(package.join("colors/wte-palette.vim"))?.contains("guibg=#111B32")
        );
        assert!(fs::read_to_string(package.join("active.txt"))?.starts_with("wte-palette\n"));
        theme.vim_scheme = "habamax".into();
        theme.code_theme = "Custom Dark".into();
        install_nvim(&mut RestorePoint::load(dir.path())?, &package, &theme)?;
        assert!(fs::read_to_string(package.join("active.txt"))?.starts_with("habamax\n"));
        let settings: serde_json::Value = serde_json::from_str(&config::vscode("{}", &theme)?)?;
        assert_eq!(
            settings,
            serde_json::json!({"workbench.colorTheme":"Custom Dark"})
        );
        Ok(())
    }

    #[test]
    fn generated_theme_references_the_render_and_exact_argb_accent() -> Result<()> {
        let theme = Theme::preset(Photo::Sienna);
        let path = r"C:\Users\Joséphine\AppData\Local\WTE\renders\a.jpg";
        let data = config::desktop_theme(&theme, path)?;
        assert_eq!(data[..2], [0xff, 0xfe]);
        let ini = String::from_utf16(
            &data[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect::<Vec<_>>(),
        )?;
        assert!(ini.contains(&format!("Wallpaper={path}\r\n")));
        assert!(ini.contains("ColorizationColor=0xFF85593F\r\n"));
        assert!(ini.contains("MTSM=DABJDKT"));
        Ok(())
    }

    #[test]
    fn presets_and_effects_render_consistently() -> Result<()> {
        use crate::{theme::Effects, wallpaper};
        for photo in Photo::ALL {
            let theme = Theme::preset(photo);
            theme.validate()?;
            let original = wallpaper::load(&theme.wallpaper)?;
            assert_eq!(original.dimensions(), (2560, 1440));
            let thumb = image::imageops::thumbnail(&original, 256, 144);
            assert_eq!(wallpaper::render(&thumb, Effects::default()), thumb);
            for fx in [
                Effects {
                    blur: 50,
                    ..Default::default()
                },
                Effects {
                    rain: 50,
                    ..Default::default()
                },
                Effects {
                    grain: 50,
                    ..Default::default()
                },
            ] {
                let out = wallpaper::render(&thumb, fx);
                assert_ne!(out, thumb);
                assert_eq!(out, wallpaper::render(&thumb, fx));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod nvim_smoke {
    use super::*;
    #[test]
    #[ignore = "requires nvim on PATH"]
    fn reported_profile_loads_with_a_reset_packpath_and_restores() -> Result<()> {
        use std::{fs, process::Command};
        let dir = tempfile::tempdir()?;
        let base = dir.path().join("profiles with spaces");
        let nvim = || {
            let mut cmd = Command::new("nvim");
            cmd.env("NVIM_APPNAME", "studio")
                .env("XDG_DATA_HOME", base.join("data"))
                .env("XDG_CONFIG_HOME", base.join("config"))
                .env("XDG_STATE_HOME", base.join("state"))
                .env("XDG_CACHE_HOME", base.join("cache"));
            cmd
        };
        let profile = crate::nvim::query(nvim())?;
        assert_eq!(profile.config, base.join("config/studio"));
        let mut point = RestorePoint::load(dir.path())?;
        let theme = Theme::preset(crate::theme::Photo::Sienna);
        fs::create_dir_all(&profile.config)?;
        let init = profile.config.join("init.vim");
        let init_text = "let &packpath = $VIMRUNTIME\ncolorscheme habamax\n";
        fs::write(&init, init_text)?;
        install_nvim(&mut point, &profile.package(), &theme)?;
        point.write(
            Component::Neovim,
            &profile.loader(),
            config::NVIM_CONFIG_LOADER.as_bytes(),
        )?;
        let verify = |scheme: &str| -> Result<()> {
            let path = dir.path().join("loaded-scheme");
            let output = nvim().args(["--headless", "-n", "-i", "NONE", "-c",
                "autocmd VimEnter * call timer_start(0, { _ -> execute(\"call writefile([get(g:, 'colors_name', '')], $WTE_RESULT) | qall!\") })"])
                .env("WTE_RESULT", &path).output()?;
            ensure!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(&path)?.trim(), scheme);
            Ok(())
        };
        verify("wte-palette")?;
        let report = restore(dir.path())?;
        assert!(report.entries.iter().all(|e| !e.error));
        assert_eq!(fs::read_to_string(init)?, init_text);
        verify("habamax")?;
        Ok(())
    }

    #[test]
    #[ignore = "requires nvim on PATH"]
    fn neovim_loads_package_at_start_and_refreshes_on_focus() -> Result<()> {
        use std::{fs, process::Command};
        let dir = tempfile::tempdir()?;
        let site = dir.path().join("site");
        let package = site.join("pack/wte/start/wte");
        install_nvim(
            &mut RestorePoint::load(dir.path())?,
            &package,
            &Theme::preset(crate::theme::Photo::Canopy),
        )?;
        let init = dir.path().join("init.vim");
        fs::write(
            &init,
            r#"let &packpath = $WTE_TEST_SITE
function! CheckWte(timer) abort
  call assert_equal('wte-palette', get(g:, 'colors_name', ''))
  call assert_equal('#121b16', tolower(synIDattr(hlID('Normal'), 'bg', 'gui')))
  call assert_equal('#fcfcfc', tolower(synIDattr(hlID('Normal'), 'fg', 'gui')))
  call assert_equal('#649a4f', tolower(synIDattr(hlID('Comment'), 'fg', 'gui')))
  call assert_equal('#ca83c3', tolower(synIDattr(hlID('@keyword.return'), 'fg', 'gui')))
  call assert_equal('#dedfa7', tolower(synIDattr(hlID('@lsp.type.function'), 'fg', 'gui')))
  call assert_equal('#d39073', tolower(synIDattr(hlID('String'), 'fg', 'gui')))
  call assert_equal('#b4d0a6', tolower(synIDattr(hlID('@lsp.type.numberLiteral'), 'fg', 'gui')))
  call writefile(['habamax', 'next'], $WTE_TEST_SITE . '/pack/wte/start/wte/active.txt')
  doautocmd FocusGained
  call assert_equal('habamax', get(g:, 'colors_name', ''))
  if len(v:errors) | call writefile(v:errors, $WTE_TEST_ERRORS) | cquit 1 | endif
  qall!
endfunction
autocmd VimEnter * call timer_start(0, function('CheckWte'))
"#,
        )?;
        let errors = dir.path().join("errors");
        let output = Command::new("nvim")
            .args(["--headless", "-n", "-i", "NONE", "-u"])
            .arg(init)
            .env("WTE_TEST_SITE", site)
            .env("WTE_TEST_ERRORS", &errors)
            .env("XDG_STATE_HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path())
            .env("XDG_CONFIG_HOME", dir.path())
            .env("XDG_CACHE_HOME", dir.path())
            .output()?;
        ensure!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(errors).unwrap_or_default()
        );
        Ok(())
    }
}
