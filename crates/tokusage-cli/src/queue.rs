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
use std::fs::{self, OpenOptions};
use std::io::Write;
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
    write_private_file(&path, text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    Ok(())
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use tokusage_core::{Client, SubmitEvent, SubmitPayload, TokenBreakdown};

    fn payload() -> SubmitPayload {
        SubmitPayload {
            client_version: "0.2.0".to_string(),
            submitted_at: Utc.with_ymd_and_hms(2026, 4, 23, 10, 30, 0).unwrap(),
            events: vec![SubmitEvent {
                source: Client::Claude,
                event_key: "claude:uuid-1".to_string(),
                event_ts: Utc.with_ymd_and_hms(2026, 4, 23, 10, 28, 0).unwrap(),
                session_key: None,
                seq: None,
                model: "claude-opus-4-7".to_string(),
                provider: "anthropic".to_string(),
                tokens: TokenBreakdown::default(),
                cost_cents: 0.0,
                raw_payload: serde_json::json!({}),
            }],
        }
    }

    #[test]
    fn enqueue_uses_private_permissions_for_queue_dir_and_file() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let previous_data_dir = std::env::var_os("TOKUSAGE_DATA_DIR");
        std::env::set_var("TOKUSAGE_DATA_DIR", tmp.path());

        let path = enqueue(&payload()).unwrap();
        let queue_dir = path.parent().unwrap();

        let dir_mode = std::fs::metadata(queue_dir).unwrap().permissions().mode() & 0o777;
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        if let Some(data_dir) = previous_data_dir {
            std::env::set_var("TOKUSAGE_DATA_DIR", data_dir);
        } else {
            std::env::remove_var("TOKUSAGE_DATA_DIR");
        }

        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
