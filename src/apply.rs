use crate::{browser::Browser, theme::Theme};
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
    path: &Path,
    backups: &Path,
    name: &str,
    edit: impl FnOnce(&str) -> Result<String>,
) -> Result<()> {
    let original = read_optional(path)?;
    let input = std::str::from_utf8(original.as_deref().unwrap_or_default())
        .context("Expected UTF-8 settings")?;
    let output = edit(input)?;
    if Some(output.as_bytes()) == original.as_deref() {
        return Ok(());
    }
    if let Some(bytes) = &original {
        atomic_write(&backups.join(name), bytes)?;
    }
    ensure!(
        read_optional(path)? == original,
        "Settings changed during apply. Retry with the latest file."
    );
    atomic_write(path, output.as_bytes())
}

#[cfg(any(windows, test))]
fn install_nvim(package: &Path, t: &Theme) -> Result<()> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let palette = config::nvim_palette(t);
    let mut hash = DefaultHasher::new();
    palette.hash(&mut hash);
    atomic_write(&package.join("colors/wte-palette.vim"), palette.as_bytes())?;
    atomic_write(
        &package.join("plugin/wte.vim"),
        config::NVIM_PLUGIN.as_bytes(),
    )?;
    let name = if t.vim_scheme.is_empty() {
        "wte-palette"
    } else {
        &t.vim_scheme
    };
    atomic_write(
        &package.join("active.txt"),
        format!("{name}\n{:016x}\n", hash.finish()).as_bytes(),
    )
}

#[cfg(any(windows, test))]
fn exovoid_registered(extensions: &Path, mode: crate::theme::Appearance) -> Result<bool> {
    let Some(index) = read_optional(&extensions.join("extensions.json"))? else {
        return Ok(false);
    };
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&index).context("Read VS Code’s extension list")?;
    let id = match mode {
        crate::theme::Appearance::Dark => "ginopioloco.exovoid",
        crate::theme::Appearance::Light => "wte-local.exovoid-light",
    };
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
    #[cfg(windows)]
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
    fn browser(&mut self, browser: Browser, root: &Path, theme: &Theme) {
        let (detail, path) = match browser.export(root, theme) {
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

    pub fn desktop(root: &Path, t: &Theme) -> Result<String> {
        let image = wallpaper::render(&wallpaper::load(&t.wallpaper)?, t.effects);
        let photo = stored_asset(root, "renders", "jpg", &wallpaper::jpeg(&image)?)?;
        let file = stored_asset(
            root,
            "desktop",
            "theme",
            &config::desktop_theme(t, photo.to_str().context("Invalid wallpaper filename")?)?,
        )?;
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
        Ok("Sent the wallpaper, accent and appearance mode to Windows Personalization.".into())
    }

    fn ensure_exovoid(
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
        let user_bin = base.data_local_dir().join("Programs/Microsoft VS Code/bin");
        let system_bin =
            env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Microsoft VS Code/bin"));
        let path = env::var_os("PATH").unwrap_or_default();
        let cli = std::iter::once(user_bin)
            .chain(system_bin)
            .chain(env::split_paths(&path))
            .filter(|dir| dir.is_absolute() && dir.join("../Code.exe").is_file())
            .map(|dir| dir.join("code.cmd"))
            .find(|file| file.is_file())
            .with_context(|| format!("Could not find the VS Code CLI. {help}"))?;
        // Both arguments are fixed. Let VS Code validate the local package and enforce its policies.
        let result = Command::new(cli)
            .current_dir(folder)
            .args(["--install-extension", package])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .output()
            .context("Start the VS Code extension manager")?;
        ensure!(
            result.status.success(),
            "VS Code could not install ExoVoid. {} {}",
            String::from_utf8_lossy(&result.stderr).trim(),
            help
        );
        Ok(true)
    }

    pub fn vscode(t: &Theme, root: &Path, backups: &Path) -> Result<String> {
        let base = directories::BaseDirs::new().context("Find roaming application data")?;
        let user = base.data_dir().join("Code/User");
        if !user.is_dir() {
            return Ok("Skipped. The default VS Code user profile was not found.".into());
        }
        let mut installed = false;
        edit_settings(
            &user.join("settings.json"),
            backups,
            "vscode-settings.json",
            |s| {
                let settings = config::vscode(s, t)?;
                if t.code_theme.trim().is_empty() {
                    installed = ensure_exovoid(root, &base, t.mode)?;
                }
                Ok(settings)
            },
        )?;
        Ok(if installed {
            "Installed the bundled ExoVoid theme and updated the default VS Code profile."
        } else {
            "Updated the default VS Code profile."
        }
        .into())
    }

    pub fn nvim(t: &Theme) -> Result<String> {
        let base = directories::BaseDirs::new().context("Find local application data")?;
        let app = env::var("NVIM_APPNAME").unwrap_or_else(|_| "nvim".into());
        ensure!(
            !app.is_empty()
                && Path::new(&app)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "Unsupported NVIM_APPNAME"
        );
        let config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.data_local_dir().to_owned())
            .join(&app);
        let data = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.data_local_dir().to_owned())
            .join(format!("{app}-data"));
        let on_path = env::var_os("PATH").is_some_and(|p| {
            env::split_paths(&p).any(|d| d.is_absolute() && d.join("nvim.exe").is_file())
        });
        if !config.is_dir() && !data.is_dir() && !on_path {
            return Ok("Skipped. Neovim was not found.".into());
        }
        let package = data.join("site/pack/wte/start/wte");
        let first = !package.join("plugin/wte.vim").exists();
        install_nvim(&package, t)?;
        Ok(if first {
            "Installed the color package. Restart open Neovim sessions once."
        } else {
            "Updated the color package. Refocus or restart Neovim."
        }
        .into())
    }

    pub fn glaze(t: &Theme, backups: &Path) -> Result<String> {
        let base = directories::BaseDirs::new().context("Find the user profile")?;
        let path = env::var_os("GLAZEWM_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".glzr/glazewm/config.yaml"));
        if !path.try_exists()? {
            return Ok("Skipped. GlazeWM configuration was not found.".into());
        }
        ensure!(path.is_absolute(), "GLAZEWM_CONFIG_PATH must be absolute.");
        edit_settings(&path, backups, "glazewm-config.yaml", |s| {
            config::glazewm(s, t)
        })?;
        if let Some(p) = env::var_os("ProgramFiles") {
            let cli = PathBuf::from(p).join("glzr.io/GlazeWM/cli/glazewm.exe");
            if cli.is_absolute() && cli.is_file() {
                let status = Command::new(cli)
                    .args(["command", "wm-reload-config"])
                    .creation_flags(0x08000000)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                if status.is_ok_and(|s| s.success()) {
                    return Ok("Updated and reloaded the window borders.".into());
                }
            }
        }
        Ok("Saved border colors. Use GlazeWM’s reload-config binding or restart it.".into())
    }
}

#[cfg(windows)]
fn desktop_components(report: &mut Report, root: &Path, t: &Theme) {
    // Files from the same apply share a backup directory. Create it only on the first actual edit.
    let backups = root.join("backups").join(crate::unique_id());
    if t.desktop {
        report.record("Windows desktop", platform::desktop(root, t));
    }
    if t.vscode {
        report.record("Visual Studio Code", platform::vscode(t, root, &backups));
    }
    if t.nvim {
        report.record("Neovim", platform::nvim(t));
    }
    if t.glaze {
        report.record("GlazeWM", platform::glaze(t, &backups));
    }
    if backups.is_dir() {
        report.record(
            "Original settings",
            Ok(format!("Backups saved in {}", backups.display())),
        );
    }
}

pub fn run(root: &Path, t: &Theme) -> Result<Report> {
    t.validate()?;
    let mut report = Report::default();
    #[cfg(windows)]
    desktop_components(&mut report, root, t);
    #[cfg(not(windows))]
    if t.desktop || t.vscode || t.nvim || t.glaze {
        report.entries.push(Entry {
            component: "Desktop integrations",
            detail: "These components require Windows.".into(),
            error: true,
            path: None,
        });
    }
    if t.chrome {
        report.browser(Browser::Chrome, root, t);
    }
    if t.firefox {
        report.browser(Browser::Firefox, root, t);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Photo;
    use std::fs;

    #[test]
    fn vscode_updates_owned_keys_and_preserves_jsonc_on_repeat() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings.json");
        let backup = dir.path().join("backup");
        let original = "\u{feff}{\r\n  // Font preference\r\n  \"editor.fontSize\": 18,\r\n  \"workbench.colorCustomizations\": {\"[Other]\": {\"focusBorder\": \"#abcdef\"}},\r\n}\r\n";
        fs::write(&path, original)?;
        for photo in Photo::ALL {
            let theme = Theme::preset(photo);
            edit_settings(&path, &backup, "vscode.json", |s| config::vscode(s, &theme))?;
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
                edit_settings(&path, dir.path(), "backup", |s| config::vscode(s, &theme)).is_err()
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
        install_nvim(&package, &theme)?;
        assert!(
            fs::read_to_string(package.join("colors/wte-palette.vim"))?.contains("guibg=#111B32")
        );
        assert!(fs::read_to_string(package.join("active.txt"))?.starts_with("wte-palette\n"));
        theme.vim_scheme = "habamax".into();
        theme.code_theme = "Custom Dark".into();
        install_nvim(&package, &theme)?;
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
        assert!(ini.contains("ColorizationColor=0xFFDE9569\r\n"));
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
    fn neovim_loads_package_at_start_and_refreshes_on_focus() -> Result<()> {
        use std::{fs, process::Command};
        let dir = tempfile::tempdir()?;
        let site = dir.path().join("site");
        let package = site.join("pack/wte/start/wte");
        install_nvim(&package, &Theme::preset(crate::theme::Photo::Canopy))?;
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
