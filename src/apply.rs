use crate::theme::Theme;
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
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

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

#[derive(Clone)]
pub struct Entry {
    pub component: &'static str,
    pub detail: String,
    pub error: bool,
}
#[derive(Default, Clone)]
pub struct Report {
    pub entries: Vec<Entry>,
}
#[cfg(windows)]
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
        Ok("Sent wallpaper and accent to Windows Personalization.".into())
    }

    pub fn vscode(t: &Theme, backups: &Path) -> Result<String> {
        let base = directories::BaseDirs::new().context("Find roaming application data")?;
        let user = base.data_dir().join("Code/User");
        if !user.is_dir() {
            return Ok("Skipped. The default VS Code user profile was not found.".into());
        }
        edit_settings(
            &user.join("settings.json"),
            backups,
            "vscode-settings.json",
            |s| config::vscode(s, t),
        )?;
        Ok("Updated the default VS Code profile.".into())
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
pub fn run(root: &Path, t: &Theme) -> Result<Report> {
    t.validate()?;
    let mut report = Report::default();
    // Files from the same apply share a backup directory. Create it only on the first actual edit.
    let backups = root.join("backups").join(crate::unique_id());
    if t.desktop {
        report.record("Windows desktop", platform::desktop(root, t));
    }
    if t.vscode {
        report.record("Visual Studio Code", platform::vscode(t, &backups));
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
    Ok(report)
}

#[cfg(not(windows))]
pub fn run(_root: &Path, _t: &Theme) -> Result<Report> {
    anyhow::bail!(
        "Desktop integration requires Windows. Theme editing and preview are available here."
    )
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
