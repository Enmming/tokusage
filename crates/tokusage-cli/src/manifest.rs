//! Data directory helpers + install manifest read/write.
//!
//! The manifest tracks everything `tokusage init` created so that
//! `tokusage self-uninstall` can reverse it precisely, without touching
//! files it didn't put there.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub fn data_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("TOKUSAGE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let dirs = directories::BaseDirs::new().context("could not determine user home directory")?;
    Ok(dirs.home_dir().join(".local/share/tokusage"))
}

pub fn queue_dir() -> Result<PathBuf> {
    let base = data_dir()?;
    create_private_dir_all(&base)?;
    let p = base.join("queue");
    create_private_dir_all(&p)?;
    Ok(p)
}

#[allow(dead_code)]
pub fn log_dir() -> Result<PathBuf> {
    let base = data_dir()?;
    create_private_dir_all(&base)?;
    let p = base.join("logs");
    create_private_dir_all(&p)?;
    Ok(p)
}

pub fn log_path() -> Result<PathBuf> {
    Ok(log_dir()?.join("submit.log"))
}

pub fn manifest_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("install-manifest.json"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallManifest {
    pub version: String,
    pub installed_at: DateTime<Utc>,
    pub binary_path: PathBuf,
    /// Absolute paths of files tokusage created. self-uninstall deletes
    /// each, ignoring already-missing entries.
    pub files: Vec<PathBuf>,
    /// True if ~/.claude/settings.json was modified with a managed hook.
    pub claude_hook_installed: bool,
}

pub fn save(manifest: &InstallManifest) -> Result<()> {
    let path = manifest_path()?;
    create_private_dir_all(&data_dir()?)?;
    create_private_dir_all(path.parent().unwrap())?;
    let text = serde_json::to_string_pretty(manifest)?;
    fs::write(&path, text)?;
    Ok(())
}

pub fn create_private_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn load() -> Result<Option<InstallManifest>> {
    let path = manifest_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let m: InstallManifest =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(m))
}

#[allow(dead_code)]
pub fn delete() -> Result<()> {
    let path = manifest_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn remove_file_best_effort(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

pub fn remove_dir_all_best_effort(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}
