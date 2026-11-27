//! Capture the active Windows personalization before handing off a WTE .theme.
use anyhow::{Context, Result, ensure};
use std::{
    ffi::{OsStr, OsString},
    fs,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr,
};
use windows::{
    Win32::{
        System::Com::{
            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize,
        },
        UI::Shell::{DWPOS_FILL, DesktopWallpaper, IDesktopWallpaper},
    },
    core::{PCWSTR, PWSTR},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    Globalization::{CP_ACP, MultiByteToWideChar},
    Graphics::{
        Dwm::DwmGetColorizationColor,
        Gdi::{COLOR_BACKGROUND, GetSysColor},
    },
    System::{
        Environment::ExpandEnvironmentStringsW,
        Registry::{
            HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
        },
        WindowsProgramming::{GetPrivateProfileStringW, WritePrivateProfileStringW},
    },
    UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CMONITORS, SPI_GETDESKWALLPAPER, SystemParametersInfoW,
    },
};

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain([0]).collect()
}
fn text(buf: &[u16]) -> OsString {
    OsString::from_wide(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

fn reg(key: &str, name: &str, flags: u32, buf: &mut [u16]) -> Result<bool> {
    let mut size = std::mem::size_of_val(buf) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            wide(name).as_ptr(),
            flags,
            ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        e => Err(std::io::Error::from_raw_os_error(e as i32))
            .with_context(|| format!("Read {key} / {name}")),
    }
}
fn string(key: &str, name: &str) -> Result<Option<OsString>> {
    let mut buf = vec![0; 32768];
    Ok(reg(key, name, RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, &mut buf)?.then(|| text(&buf)))
}
fn number(key: &str, name: &str) -> Result<Option<u32>> {
    let mut buf = [0; 2];
    Ok(reg(key, name, RRF_RT_REG_DWORD, &mut buf)?
        .then(|| u32::from(buf[0]) | u32::from(buf[1]) << 16))
}
fn expanded(path: &OsStr) -> Result<PathBuf> {
    let mut buf = vec![0; 32768];
    let n = unsafe {
        ExpandEnvironmentStringsW(wide(path).as_ptr(), buf.as_mut_ptr(), buf.len() as u32)
    };
    ensure!(
        n > 0 && n as usize <= buf.len(),
        "Invalid desktop theme path"
    );
    Ok(PathBuf::from(text(&buf)))
}
struct Ini(Vec<u16>);
impl Ini {
    fn set(&self, section: &str, key: &str, value: impl AsRef<OsStr>) -> Result<()> {
        let result = unsafe {
            WritePrivateProfileStringW(
                wide(section).as_ptr(),
                wide(key).as_ptr(),
                wide(value).as_ptr(),
                self.0.as_ptr(),
            )
        };
        ensure!(result != 0, "Could not capture {section} / {key}");
        Ok(())
    }
    fn get(&self, section: &str, key: &str) -> OsString {
        let mut buf = vec![0; 32768];
        unsafe {
            GetPrivateProfileStringW(
                wide(section).as_ptr(),
                wide(key).as_ptr(),
                ptr::null(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                self.0.as_ptr(),
            );
        }
        text(&buf)
    }
}

// Windows theme packs may reference files beside the source .theme. Keep those
// references valid when saving the snapshot in WTE's own directory.
fn copy_theme(source: &Path, target: &Path) -> Result<()> {
    let bytes = fs::read(source)?;
    let content = if let Some(raw) = bytes.strip_prefix(&[0xff, 0xfe]) {
        ensure!(raw.len() % 2 == 0, "Incomplete UTF-16 Windows theme");
        String::from_utf16(
            &raw.as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect::<Vec<_>>(),
        )?
    } else if let Ok(s) = std::str::from_utf8(&bytes) {
        s.trim_start_matches('\u{feff}').to_owned()
    } else {
        let size = i32::try_from(bytes.len())?;
        let mut words = vec![0; bytes.len()];
        let n = unsafe {
            MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), size, words.as_mut_ptr(), size)
        };
        ensure!(n > 0, "Could not read the Windows theme encoding");
        String::from_utf16(&words[..n as usize])?
    };
    let directory = source.parent().context("Locate the active theme folder")?;
    let mut result = String::new();
    for line in content.lines() {
        let rebased = line.split_once('=').and_then(|(key, value)| {
            let path = Path::new(value.trim());
            if path.as_os_str().is_empty() || !path.is_relative() || value.contains('%') {
                return None;
            }
            let path = directory.join(path);
            path.exists().then(|| format!("{key}={}", path.display()))
        });
        result.push_str(rebased.as_deref().unwrap_or(line));
        result.push_str("\r\n");
    }
    let bytes = std::iter::once(0xfeff)
        .chain(result.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    fs::write(target, bytes)?;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MonitorWallpaper {
    id: String,
    image: PathBuf,
}

fn with_desktop<T>(f: impl FnOnce(&IDesktopWallpaper) -> Result<T>) -> Result<T> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let result = (|| {
        let desktop = unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)? };
        f(&desktop)
    })();
    unsafe { CoUninitialize() };
    result
}

fn owned_string(value: PWSTR) -> Result<String> {
    if value.is_null() {
        return Ok(String::new());
    }
    // Both string-returning IDesktopWallpaper methods transfer this allocation.
    let text = unsafe { value.to_string() };
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    Ok(text?)
}

fn current_monitors() -> Result<Vec<MonitorWallpaper>> {
    if unsafe { GetSystemMetrics(SM_CMONITORS) } <= 1 {
        return Ok(Vec::new());
    }
    with_desktop(|desktop| {
        (0..unsafe { desktop.GetMonitorDevicePathCount()? })
            .map(|i| {
                let id = owned_string(unsafe { desktop.GetMonitorDevicePathAt(i)? })?;
                let image =
                    owned_string(unsafe { desktop.GetWallpaper(PCWSTR(wide(&id).as_ptr()))? })?;
                Ok(MonitorWallpaper {
                    id,
                    image: image.into(),
                })
            })
            .collect()
    })
}

fn save_wallpaper(source: &Path, mut copy: PathBuf) -> Result<PathBuf> {
    if source.as_os_str().is_empty() {
        return Ok(PathBuf::new());
    }
    copy.set_extension(source.extension().unwrap_or_else(|| OsStr::new("jpg")));
    fs::copy(source, &copy)
        .with_context(|| format!("Save the current wallpaper {}", source.display()))?;
    Ok(copy)
}

fn snapshot_wallpaper(directory: &Path, mut monitors: Vec<MonitorWallpaper>) -> Result<PathBuf> {
    if let Some(first) = monitors.first() {
        if monitors.iter().all(|m| m.image == first.image) {
            return save_wallpaper(&first.image, directory.join("wallpaper"));
        }
        for (i, monitor) in monitors.iter_mut().enumerate() {
            monitor.image = save_wallpaper(&monitor.image, directory.join(format!("monitor-{i}")))?;
        }
        fs::write(
            directory.join("monitors.json"),
            serde_json::to_vec(&monitors)?,
        )?;
        return monitors
            .into_iter()
            .find(|m| !m.image.as_os_str().is_empty())
            .map(|m| m.image)
            .context("Locate the saved monitor wallpaper");
    }
    let mut buf = vec![0; 32768];
    ensure!(
        unsafe {
            SystemParametersInfoW(
                SPI_GETDESKWALLPAPER,
                buf.len() as u32,
                buf.as_mut_ptr().cast(),
                0,
            )
        } != 0,
        "Read the active Windows wallpaper"
    );
    save_wallpaper(Path::new(&text(&buf)), directory.join("wallpaper"))
}

const THEMES: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes";

fn current_theme() -> Result<Option<PathBuf>> {
    string(THEMES, "CurrentTheme")?
        .map(|v| expanded(&v))
        .transpose()
}

fn is_current(requested: &Path) -> Result<bool> {
    let Some(active) = current_theme()? else {
        return Ok(false);
    };
    if active == requested {
        return Ok(true);
    }
    // Windows may rewrite the selected file as Custom.theme, resolving indirect
    // display names and changing the encoding. Compare the effective fields too.
    if !active.try_exists()? {
        return Ok(false);
    }
    same_theme_file(&active, requested)
}

fn same_theme_file(active: &Path, requested: &Path) -> Result<bool> {
    if fs::read(active)? == fs::read(requested)? {
        return Ok(true);
    }
    let actual = Ini(wide(active));
    let wanted = Ini(wide(requested));
    let wallpaper = wanted.get(r"Control Panel\Desktop", "Wallpaper");
    if wallpaper.is_empty() || !Path::new(&wallpaper).is_absolute() {
        return Ok(false);
    }
    if Path::new(&actual.get(r"Control Panel\Desktop", "Wallpaper")) != Path::new(&wallpaper) {
        return Ok(false);
    }
    for key in [
        "ColorizationColor",
        "AppMode",
        "SystemMode",
        "AutoColorization",
    ] {
        let expected = wanted.get("VisualStyles", key);
        if !expected.is_empty()
            && !actual
                .get("VisualStyles", key)
                .eq_ignore_ascii_case(&expected)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn wait_for_theme(theme: &Path) -> Result<bool> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if is_current(theme)? {
            return Ok(true);
        }
        if std::time::Instant::now() >= until {
            return Ok(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

pub fn activate(
    theme: &Path,
    wallpaper: &Path,
    name: &str,
    open: impl FnOnce(&Path) -> Result<()>,
) -> Result<String> {
    if is_current(theme).unwrap_or(false)
        && with_desktop(|desktop| {
            let active = owned_string(unsafe { desktop.GetWallpaper(PCWSTR::null())? })?;
            Ok(Path::new(&active) == wallpaper)
        })
        .unwrap_or(false)
    {
        return Ok(format!("WTE {name} is already active."));
    }
    let handoff = open(theme);
    if handoff.is_ok() && wait_for_theme(theme).unwrap_or(false) {
        return Ok(format!("Windows selected WTE {name}."));
    }
    // Wallpaper has its own documented setter. The shell owns full-theme selection.
    with_desktop(|desktop| {
        unsafe {
            desktop.SetPosition(DWPOS_FILL)?;
            desktop.SetWallpaper(PCWSTR::null(), PCWSTR(wide(wallpaper).as_ptr()))?;
        }
        Ok(())
    }).with_context(|| format!("Windows did not confirm the theme or accept its wallpaper. Select WTE {name} in Personalization ({})", theme.display()))?;
    if let Err(error) = handoff {
        anyhow::bail!(
            "Wallpaper applied. Opening the theme failed: {error:#}. Select WTE {name} in Personalization for the accent and mode."
        );
    }
    Ok(format!(
        "Wallpaper applied. Windows has not confirmed the full theme. If needed, select WTE {name} in Personalization for its accent and mode."
    ))
}

pub fn restore(theme: &Path, open: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    if !is_current(theme)? {
        open(theme)?;
        ensure!(
            wait_for_theme(theme)?,
            "Windows has not confirmed the saved theme. Open {} in Personalization, then retry Restore.",
            theme.display()
        );
    }
    let Some(data) = crate::store::read_optional(&theme.with_file_name("monitors.json"))? else {
        return Ok(());
    };
    let monitors: Vec<MonitorWallpaper> = serde_json::from_slice(&data)?;
    let marker = PathBuf::from(Ini(wide(theme)).get(r"Control Panel\Desktop", "Wallpaper"));
    with_desktop(|desktop| {
        // The shell applies .theme files asynchronously. Wait until Windows
        // reports our snapshot wallpaper before replacing individual monitors.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let current = owned_string(unsafe { desktop.GetWallpaper(PCWSTR::null())? })?;
            if !current.is_empty() && Path::new(&current) == marker {
                break;
            }
            ensure!(
                std::time::Instant::now() < until,
                "Windows has not activated the saved desktop. Finish applying it in Personalization, then retry Restore."
            );
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        for monitor in monitors {
            unsafe {
                desktop.SetWallpaper(
                    PCWSTR(wide(monitor.id).as_ptr()),
                    PCWSTR(wide(monitor.image).as_ptr()),
                )?;
            }
        }
        Ok(())
    })
}

pub fn snapshot(root: &Path) -> Result<PathBuf> {
    const DESKTOP: &str = r"Control Panel\Desktop";
    let folder = root.join("restore-desktop");
    fs::create_dir_all(&folder)?;
    let pending = tempfile::Builder::new()
        .prefix("desktop-")
        .tempdir_in(folder)?;
    let directory = pending.path();
    let target = directory.join("previous.theme");
    let source = current_theme()?;
    let base = source
        .filter(|p| p.is_file())
        .or_else(|| {
            std::env::var_os("SystemRoot")
                .map(|p| PathBuf::from(p).join("Resources/Themes/aero.theme"))
                .filter(|p| p.is_file())
        })
        .context(
            "Cannot capture the active Windows theme. Save it in Personalization and apply again.",
        )?;
    copy_theme(&base, &target)?;
    let ini = Ini(wide(&target));
    // For a saved slideshow or Spotlight, keep the source and rotation settings.
    let dynamic =
        !ini.get("Slideshow", "Interval").is_empty() || ini.get(DESKTOP, "SpotlightEnabled") == "1";
    if !dynamic {
        ini.set(
            DESKTOP,
            "Wallpaper",
            snapshot_wallpaper(directory, current_monitors()?)?,
        )?;
    }
    for key in ["WallpaperStyle", "TileWallpaper"] {
        if let Some(value) = string(DESKTOP, key)? {
            ini.set(DESKTOP, key, value)?;
        }
    }
    let c = unsafe { GetSysColor(COLOR_BACKGROUND) };
    ini.set(
        r"Control Panel\Colors",
        "Background",
        format!("{} {} {}", c & 255, (c >> 8) & 255, (c >> 16) & 255),
    )?;
    let mut color = 0;
    let mut opaque = 0;
    ensure!(
        unsafe { DwmGetColorizationColor(&mut color, &mut opaque) } >= 0,
        "Read Windows accent color"
    );
    ini.set(
        "VisualStyles",
        "ColorizationColor",
        format!("0x{color:08X}"),
    )?;
    if let Some(auto) = number(DESKTOP, "AutoColorization")? {
        ini.set("VisualStyles", "AutoColorization", auto.to_string())?;
    }
    for (key, field) in [
        ("AppsUseLightTheme", "AppMode"),
        ("SystemUsesLightTheme", "SystemMode"),
    ] {
        if let Some(value) = number(&format!(r"{THEMES}\Personalize"), key)? {
            ini.set(
                "VisualStyles",
                field,
                if value == 0 { "Dark" } else { "Light" },
            )?;
        }
    }
    let _ = pending.keep();
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_rewritten_theme_and_checks_its_accent_and_mode() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let wanted = dir.path().join("selected.theme");
        let active = dir.path().join("Custom.theme");
        let data = "[Theme]\r\nDisplayName=WTE Demo\r\n[Control Panel\\Desktop]\r\nWallpaper=C:\\Pictures\\a.jpg\r\n[VisualStyles]\r\nColorizationColor=0xff223344\r\nAppMode=Dark\r\nSystemMode=Dark\r\n";
        fs::write(&wanted, data)?;
        fs::write(&active, data.replace("WTE Demo", "Custom"))?;
        assert!(same_theme_file(&active, &wanted)?);
        for (from, to) in [
            ("Dark", "Light"),
            ("ff223344", "ff000000"),
            ("a.jpg", "b.jpg"),
        ] {
            fs::write(&active, data.replace(from, to))?;
            assert!(!same_theme_file(&active, &wanted)?);
        }
        Ok(())
    }

    #[test]
    fn preserves_distinct_per_monitor_wallpapers_when_windows_replaces_the_sources() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let monitors: Vec<_> = ["left", "right"]
            .into_iter()
            .map(|id| MonitorWallpaper {
                id: id.into(),
                image: dir.path().join(format!("{id}.bmp")),
            })
            .collect();
        for m in &monitors {
            fs::write(&m.image, &m.id)?;
        }
        let primary = snapshot_wallpaper(dir.path(), monitors)?;
        let saved: Vec<MonitorWallpaper> =
            serde_json::from_slice(&fs::read(dir.path().join("monitors.json"))?)?;
        for m in saved {
            fs::remove_file(dir.path().join(format!("{}.bmp", m.id)))?;
            assert_eq!(fs::read_to_string(m.image)?, m.id);
        }
        assert_eq!(fs::read(primary)?, b"left");
        Ok(())
    }

    #[test]
    fn snapshot_rebases_relative_assets_and_accepts_unicode_wallpaper_paths() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let base = dir.path().join("original.theme");
        let target = dir.path().join("backup.theme");
        fs::create_dir(dir.path().join("DesktopBackground"))?;
        fs::write(
            &base,
            "[Theme]\r\nDisplayName=Family\r\n[Slideshow]\r\nImagesRootPath=DesktopBackground\r\n",
        )?;
        copy_theme(&base, &target)?;
        let ini = Ini(wide(&target));
        assert_eq!(
            ini.get("Slideshow", "ImagesRootPath"),
            dir.path().join("DesktopBackground").as_os_str()
        );
        ini.set(r"Control Panel\Desktop", "Wallpaper", r"C:\背景\雨.jpg")?;
        assert_eq!(
            ini.get(r"Control Panel\Desktop", "Wallpaper"),
            r"C:\背景\雨.jpg"
        );
        assert_eq!(ini.get("Theme", "DisplayName"), "Family");
        Ok(())
    }
}
