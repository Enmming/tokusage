//! macOS launchd integration.
//!
//! `install()` writes a user LaunchAgent plist at
//! `~/Library/LaunchAgents/com.gd.tokusage.plist` that invokes
//! `tokusage submit` every 30 minutes, then loads it with launchctl.
//! `uninstall()` reverses both.

use anyhow::{Context, Result};
use plist::Value;
use std::path::PathBuf;
use std::process::Command;

pub const LABEL: &str = "com.gd.tokusage";
/// Every 30 minutes.
pub const INTERVAL_SECS: i64 = 1800;

pub fn plist_path() -> Result<PathBuf> {
    let dirs = directories::BaseDirs::new()
        .context("could not determine user home directory")?;
    Ok(dirs
        .home_dir()
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", LABEL)))
}

pub fn write_plist(binary_path: &std::path::Path, log_path: &std::path::Path) -> Result<PathBuf> {
    let path = plist_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;

    let mut dict = plist::Dictionary::new();
    dict.insert("Label".to_string(), Value::String(LABEL.to_string()));

    dict.insert(
        "ProgramArguments".to_string(),
        Value::Array(vec![
            Value::String(binary_path.to_string_lossy().to_string()),
            Value::String("submit".to_string()),
        ]),
    );

    dict.insert(
        "StartInterval".to_string(),
        Value::Integer(INTERVAL_SECS.into()),
    );
    dict.insert("RunAtLoad".to_string(), Value::Boolean(true));
    dict.insert(
        "StandardOutPath".to_string(),
        Value::String(log_path.to_string_lossy().to_string()),
    );
    dict.insert(
        "StandardErrorPath".to_string(),
        Value::String(log_path.to_string_lossy().to_string()),
    );
    // Populate PATH so `tokusage` and `xattr` etc. resolve under launchd.
    let mut env = plist::Dictionary::new();
    env.insert(
        "PATH".to_string(),
        Value::String(
            "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        ),
    );
    dict.insert("EnvironmentVariables".to_string(), Value::Dictionary(env));

    plist::to_file_xml(&path, &Value::Dictionary(dict))
        .with_context(|| format!("writing {}", path.display()))?;

    Ok(path)
}

pub fn launchctl_load(plist: &std::path::Path) -> Result<()> {
    let status = Command::new("launchctl")
        .arg("load")
        .arg("-w")
        .arg(plist)
        .status()
        .context("failed to run launchctl load")?;
    if !status.success() {
        anyhow::bail!("launchctl load exited with {}", status);
    }
    Ok(())
}

pub fn launchctl_unload(plist: &std::path::Path) -> Result<()> {
    // Unload is a best-effort, even if plist doesn't exist or is already unloaded.
    let _ = Command::new("launchctl")
        .arg("unload")
        .arg("-w")
        .arg(plist)
        .status();
    Ok(())
}
