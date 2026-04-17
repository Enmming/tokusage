//! Disk-backed retry queue for submit payloads.
//!
//! When a POST fails (network error, 5xx, etc.), we drop the serialized
//! payload into `~/.local/share/tokusage/queue/<timestamp>-<uuid>.json` and
//! retry it on the next `tokusage submit`. Successful submits delete the
//! queued file. Individual files that fail to parse (corruption, schema
//! drift after upgrade) are moved aside to `queue/poison/` so they stop
//! blocking the queue.

use crate::manifest::queue_dir;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tokusage_core::SubmitPayload;

pub fn enqueue(payload: &SubmitPayload) -> Result<PathBuf> {
    let dir = queue_dir()?;
    let name = format!(
        "{}-{}.json",
        chrono::Utc::now().format("%Y%m%dT%H%M%S"),
        uuid::Uuid::new_v4()
    );
    let path = dir.join(name);
    let text = serde_json::to_string(payload)?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// List queued files oldest first.
pub fn list() -> Result<Vec<PathBuf>> {
    let dir = queue_dir()?;
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    entries.sort();
    Ok(entries)
}

pub fn load(path: &std::path::Path) -> Result<SubmitPayload> {
    let text = fs::read_to_string(path)?;
    let payload: SubmitPayload = serde_json::from_str(&text)?;
    Ok(payload)
}

pub fn remove(path: &std::path::Path) -> Result<()> {
    fs::remove_file(path)?;
    Ok(())
}

pub fn quarantine(path: &std::path::Path) -> Result<()> {
    let poison = queue_dir()?.join("poison");
    fs::create_dir_all(&poison)?;
    let name = path.file_name().context("queue file has no name")?;
    fs::rename(path, poison.join(name))?;
    Ok(())
}
