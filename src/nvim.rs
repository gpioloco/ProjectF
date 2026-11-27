//! Resolve the same profile that Windows Neovim will load.
use anyhow::{Result, ensure};
use std::{
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, PartialEq)]
pub struct Profile {
    pub config: PathBuf,
    pub data: PathBuf,
}
impl Profile {
    pub fn package(&self) -> PathBuf {
        self.data.join("site/pack/wte/start/wte")
    }
    pub fn loader(&self) -> PathBuf {
        self.config.join("after/plugin/wte-theme-engine.lua")
    }
}

pub fn query(mut command: Command) -> Result<Profile> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let paths = "lua io.stdout:write(vim.fn.json_encode({vim.fn.stdpath('config'),vim.fn.stdpath('data')}))";
    let out = command
        .args([
            "--headless",
            "-u",
            "NONE",
            "-i",
            "NONE",
            "-n",
            "-c",
            paths,
            "-c",
            "qa!",
        ])
        .stdin(Stdio::null())
        .output()?;
    ensure!(
        out.status.success(),
        "Neovim could not report its profile: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let [config, data]: [PathBuf; 2] = serde_json::from_slice(&out.stdout)?;
    ensure!(
        config.is_absolute() && data.is_absolute(),
        "Neovim reported relative profile paths"
    );
    Ok(Profile { config, data })
}

fn appdata_profile(
    app: &str,
    local: &Path,
    roaming: &Path,
    config_home: Option<&Path>,
    data_home: Option<&Path>,
) -> Result<Option<Profile>> {
    ensure!(
        !app.is_empty()
            && Path::new(app)
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Invalid NVIM_APPNAME"
    );
    let configs: Vec<_> = config_home
        .map_or_else(|| vec![local, roaming], |dir| vec![dir])
        .into_iter()
        .map(|dir| dir.join(app))
        .collect();
    let config = configs
        .iter()
        .find(|dir| dir.join("init.lua").is_file() || dir.join("init.vim").is_file())
        .or_else(|| configs.iter().find(|dir| dir.is_dir()))
        .unwrap_or(&configs[0]);
    let roots = data_home.map_or_else(|| vec![local, roaming], |dir| vec![dir]);
    let data_dirs: Vec<_> = roots
        .iter()
        .map(|dir| dir.join(format!("{app}-data")))
        .collect();
    let data = data_dirs
        .iter()
        .find(|dir| dir.is_dir())
        .unwrap_or(&data_dirs[0]);
    Ok((config.is_dir() || data.is_dir()).then(|| Profile {
        config: config.clone(),
        data: data.clone(),
    }))
}

#[cfg(windows)]
pub fn locate() -> Result<Option<Profile>> {
    use anyhow::Context;
    use std::env;
    fn env_path(name: &str) -> Option<PathBuf> {
        env::var_os(name)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }
    let base = directories::BaseDirs::new().context("Locate Windows AppData")?;
    let local = env_path("LOCALAPPDATA").unwrap_or_else(|| base.data_local_dir().into());
    let roaming = env_path("APPDATA").unwrap_or_else(|| base.data_dir().into());
    let mut bins: Vec<_> = env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .filter(|dir| dir.is_absolute())
        .collect();
    bins.extend([
        local.join("Programs/Neovim/bin"),
        local.join("Microsoft/WinGet/Links"),
    ]);
    if let Some(programs) = env_path("ProgramFiles") {
        bins.push(programs.join("Neovim/bin"));
    }
    if let Ok(packages) = std::fs::read_dir(local.join("Microsoft/WinGet/Packages")) {
        for item in packages.flatten().filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("Neovim.Neovim_")
        }) {
            bins.push(item.path().join("nvim-win64/bin"));
        }
    }
    let mut failed_query = None;
    for exe in bins
        .into_iter()
        .map(|dir| dir.join("nvim.exe"))
        .filter(|p| p.is_file())
    {
        match query(Command::new(&exe))
            .with_context(|| format!("Read the active profile from {}", exe.display()))
        {
            Ok(profile) => return Ok(Some(profile)),
            Err(e) => {
                failed_query.get_or_insert(e);
            }
        }
    }
    let config_home = env_path("XDG_CONFIG_HOME");
    let data_home = env_path("XDG_DATA_HOME");
    ensure!(
        config_home
            .iter()
            .chain(&data_home)
            .all(|p| p.is_absolute()),
        "Use absolute XDG profile directories"
    );
    let fallback = appdata_profile(
        &env::var("NVIM_APPNAME").unwrap_or_else(|_| "nvim".into()),
        &local,
        &roaming,
        config_home.as_deref(),
        data_home.as_deref(),
    )?;
    match (fallback, failed_query) {
        (None, Some(e)) => Err(e),
        (profile, _) => Ok(profile),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn searches_appdata_and_prefers_explicit_profiles() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let local = dir.path().join("Local");
        let roaming = dir.path().join("Roaming");
        let config = roaming.join("work");
        let data = local.join("work-data");
        std::fs::create_dir_all(&config)?;
        std::fs::write(config.join("init.lua"), b"")?;
        std::fs::create_dir_all(&data)?;
        let profile = appdata_profile("work", &local, &roaming, None, None)?.unwrap();
        assert_eq!(profile, Profile { config, data });
        assert!(
            appdata_profile("work", &local, &roaming, Some(dir.path()), Some(dir.path()))?
                .is_none()
        );
        assert!(appdata_profile("../other", &local, &roaming, None, None).is_err());
        Ok(())
    }
}
