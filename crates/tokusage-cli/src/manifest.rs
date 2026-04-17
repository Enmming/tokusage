//! Data directory helpers used by submit, queue, and (later) manifest.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

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

// tiny hex encoder to avoid pulling in the hex crate
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
