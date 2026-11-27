use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn nsis() -> PathBuf {
    #[cfg(windows)]
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(dir) = env::var_os(key) {
            let exe = PathBuf::from(dir).join("NSIS").join("makensis.exe");
            if exe.is_file() {
                return exe;
            }
        }
    }
    "makensis".into()
}

fn define(key: &str, value: &OsStr) -> OsString {
    let prefix = if cfg!(windows) { "/D" } else { "-D" };
    let mut arg = OsString::from(format!("{prefix}{key}="));
    arg.push(value);
    arg
}

fn main() -> Result<()> {
    let target = if cfg!(all(windows, target_env = "msvc")) {
        "x86_64-pc-windows-msvc"
    } else {
        "x86_64-pc-windows-gnu"
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let metadata = Command::new(&cargo)
        .current_dir(root)
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .output()?;
    ensure!(
        metadata.status.success(),
        "Cargo metadata failed: {}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: Value = serde_json::from_slice(&metadata.stdout)?;
    let packages = metadata["packages"]
        .as_array()
        .context("Missing Cargo packages")?;
    let version = packages
        .iter()
        .find(|p| p["name"] == "wte")
        .and_then(|p| p["version"].as_str())
        .context("Missing WTE package version")?;
    let target_dir = Path::new(
        metadata["target_directory"]
            .as_str()
            .context("Missing Cargo target directory")?,
    );
    let compiler = nsis();
    let version_flag = if cfg!(windows) {
        "/VERSION"
    } else {
        "-VERSION"
    };
    ensure!(
        Command::new(&compiler)
            .arg(version_flag)
            .status()
            .context("Install NSIS 3 and make makensis available to Cargo")?
            .success(),
        "Could not run makensis"
    );
    let built = Command::new(cargo)
        .current_dir(root)
        .args([
            "build",
            "--locked",
            "--release",
            "-p",
            "wte",
            "--bin",
            "wte",
            "--target",
            target,
        ])
        .status()?;
    ensure!(built.success(), "Windows app build failed");
    let dist = target_dir.join("dist");
    fs::create_dir_all(&dist)?;
    let setup = dist.join(format!("WTE-{version}-windows-x64-setup.exe"));
    let exe = target_dir.join(target).join("release/wte.exe");
    let result = Command::new(compiler)
        .current_dir(root)
        .arg(define("VERSION", version.as_ref()))
        .arg(define("WTE_EXE", exe.as_os_str()))
        .arg(define("SOURCE", root.as_os_str()))
        .arg(define("OUTPUT", setup.as_os_str()))
        .arg(root.join("installer/wte.nsi"))
        .status()?;
    ensure!(result.success(), "Installer compilation failed");
    println!("Built {}", setup.display());
    Ok(())
}
