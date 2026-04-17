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
    let dirs = directories::BaseDirs::new()
        .context("could not determine user home directory")?;
    Ok(dirs.home_dir().join(".local/share/tokusage"))
}

pub fn queue_dir() -> Result<PathBuf> {
    let p = data_dir()?.join("queue");
    fs::create_dir_all(&p)?;
    Ok(p)
}

#[allow(dead_code)]
pub fn log_dir() -> Result<PathBuf> {
    let p = data_dir()?.join("logs");
    fs::create_dir_all(&p)?;
    Ok(p)
}

pub fn log_path() -> Result<PathBuf> {
    Ok(log_dir()?.join("submit.log"))
}

pub fn manifest_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("install-manifest.json"))
}

/// Stable per-machine identifier that does not leak the username to the
/// server: `sha256(username + hostname)`, truncated to 16 hex chars.
pub fn host_id() -> String {
    use sha2::{Digest, Sha256};
    let user = std::env::var("USER").unwrap_or_default();
    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(user.as_bytes());
    hasher.update(b":");
    hasher.update(host.as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push(to_hex(b >> 4));
            s.push(to_hex(b & 0x0f));
        }
        s
    }
    fn to_hex(v: u8) -> char {
        match v {
            0..=9 => (b'0' + v) as char,
            10..=15 => (b'a' + v - 10) as char,
            _ => '?',
        }
    }
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
    fs::create_dir_all(path.parent().unwrap())?;
    let text = serde_json::to_string_pretty(manifest)?;
    fs::write(&path, text)?;
    Ok(())
}

pub fn load() -> Result<Option<InstallManifest>> {
    let path = manifest_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let m: InstallManifest = serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))?;
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
