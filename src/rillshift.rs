//! Rillshift profile selection and reload through its installed CLI.
use crate::{
    apply::edit_settings,
    config,
    restore::{Component, RestorePoint},
    theme::Theme,
};
use anyhow::{Context, Result, bail, ensure};
use directories::BaseDirs;
use serde::Deserialize;
use serde_json::Value;
use std::{
    env, fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Reply {
    success: bool,
    data: Option<Value>,
    error: Option<String>,
}

fn request(cli: &Path, args: &[&str]) -> Result<Value> {
    let mut child = Command::new(cli)
        .args(args)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let start = Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() >= Duration::from_secs(4) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Rillshift did not respond within four seconds");
        }
        thread::sleep(Duration::from_millis(30));
    }
    let output = child.wait_with_output()?;
    ensure!(
        output.status.success(),
        "Rillshift CLI could not reach the manager"
    );
    let reply: Reply =
        serde_json::from_slice(&output.stdout).context("Read Rillshift CLI response")?;
    ensure!(
        reply.success,
        "{}",
        reply
            .error
            .as_deref()
            .unwrap_or("Rillshift rejected the command")
    );
    Ok(reply.data.unwrap_or(Value::Null))
}

struct Running {
    cli: PathBuf,
    profile: PathBuf,
}

fn running(base: &BaseDirs) -> Result<Option<Running>> {
    let search = env::var_os("PATH").unwrap_or_default();
    let cli = std::iter::once(base.data_local_dir().join("Programs/Rillshift"))
        .chain(env::split_paths(&search).filter(|dir| dir.is_absolute()))
        .map(|dir| dir.join("rillshift-cli.exe"))
        .find(|path| path.is_file());
    let Some(cli) = cli else { return Ok(None) };
    let Ok(meta) = request(&cli, &["query", "app-metadata"]) else {
        return Ok(None);
    };
    // A compatible IPC endpoint may belong to GlazeWM. Only reload Rillshift.
    if meta["product"] != "Rillshift" {
        return Ok(None);
    }
    let profile = PathBuf::from(
        meta["configPath"]
            .as_str()
            .context("Rillshift did not report its active profile")?,
    );
    ensure!(
        profile.is_absolute(),
        "Rillshift reported a relative profile path"
    );
    Ok(Some(Running { cli, profile }))
}

pub fn is_running() -> bool {
    BaseDirs::new()
        .and_then(|base| running(&base).ok().flatten())
        .is_some()
}

fn reload(base: &BaseDirs, paths: &[PathBuf]) -> Result<String> {
    let Some(active) = running(base)? else {
        return Ok("Rillshift will read the saved colors at startup.".into());
    };
    let profile = fs::canonicalize(&active.profile)?;
    if !paths
        .iter()
        .any(|path| fs::canonicalize(path).is_ok_and(|p| p == profile))
    {
        return Ok("The running manager uses a different profile.".into());
    }
    if request(&active.cli, &["query", "paused"])?
        .as_bool()
        .context("Unexpected Rillshift pause status")?
    {
        return Ok("Resume Rillshift and reload its profile to see the colors.".into());
    }
    request(&active.cli, &["command", "wm-reload-config"])?;
    Ok("Reloaded Rillshift.".into())
}

pub fn apply(point: &mut RestorePoint, theme: &Theme, backups: &Path) -> Result<String> {
    let base = BaseDirs::new().context("Locate the Rillshift profile")?;
    let path = match running(&base)? {
        Some(active) => active.profile,
        None => env::var_os("RILLSHIFT_CONFIG_PATH")
            .or_else(|| env::var_os("PROJECTWM_CONFIG_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".rillshift/config.yaml")),
    };
    ensure!(path.is_absolute(), "Use an absolute Rillshift config path");
    if !path.try_exists()? {
        return Ok(format!(
            "Skipped. No profile at {}. Start Rillshift once to create it.",
            path.display()
        ));
    }
    edit_settings(
        point,
        Component::Rillshift,
        &path,
        backups,
        "rillshift-config.yaml",
        |input| config::rillshift(input, theme),
    )?;
    let status = reload(&base, std::slice::from_ref(&path))
        .context("Colors saved. Reload Rillshift to apply them")?;
    Ok(format!(
        "Saved border and bar accent colors in {}. {status}",
        path.display()
    ))
}

pub fn reload_restored(paths: &[PathBuf]) -> Result<String> {
    let base = BaseDirs::new().context("Locate Rillshift")?;
    let status = reload(&base, paths).context("Profile restored. Reload Rillshift to apply it")?;
    Ok(format!("Restored the original profile. {status}"))
}
