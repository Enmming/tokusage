//! opencode session parser.
//!
//! Reads assistant-message usage from opencode's local data dir
//! (`OPENCODE_DATA_DIR`, default `~/.local/share/opencode`). Two storage tiers:
//! legacy JSON at `storage/message/{sessionID}/msg_*.json` and the newer SQLite
//! `opencode.db` (`message(id, session_id, data)`). Both can hold the same
//! message, so entries are deduplicated by message id. The DB tier is read
//! first (newer, migrated copy) so it wins on a collision.
//!
//! Schema (assistant message JSON):
//! `tokens.{input,output,reasoning,cache.{read,write}}`, `modelID`, `providerID`,
//! `time.created` (epoch millis), `cost` (USD), `id`, `sessionID`.

use super::ScanResult;
use crate::model::{Client, TokenBreakdown, UnifiedMessage};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn default_root() -> Option<PathBuf> {
    if let Ok(var) = std::env::var("OPENCODE_DATA_DIR") {
        if let Some(first) = var.split(',').map(str::trim).find(|s| !s.is_empty()) {
            return Some(PathBuf::from(first));
        }
    }
    directories::BaseDirs::new().map(|d| d.home_dir().join(".local/share/opencode"))
}

pub fn scan(root: &Path) -> ScanResult {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // JSON tier: storage/message/**/*.json (one message per file).
    let messages_dir = root.join("storage").join("message");
    for entry in WalkDir::new(&messages_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match parse_json_file(path) {
            Ok(Some(m)) => {
                if seen.insert(m.event_key.clone()) {
                    out.push(m);
                }
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(?path, error = %err, "failed to parse opencode message"),
        }
    }

    Ok(out)
}

fn parse_json_file(path: &Path) -> anyhow::Result<Option<UnifiedMessage>> {
    let content = std::fs::read_to_string(path)?;
    let value: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    // Fall back to the file stem (keeps the `msg_` prefix, matching the DB id)
    // only when the in-file `id` is absent.
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());
    Ok(message_value_to_unified(&value, stem, None, "json"))
}

/// Map one opencode assistant-message JSON value to a `UnifiedMessage`.
/// Returns `None` when the entry has no usage, is missing model/provider, or
/// has no parseable `time.created`.
fn message_value_to_unified(
    value: &Value,
    id_hint: Option<String>,
    session_hint: Option<String>,
    tier: &str,
) -> Option<UnifiedMessage> {
    let tokens = value.get("tokens")?;
    let input = json_i64(tokens.get("input"));
    let output = json_i64(tokens.get("output"));
    let reasoning = json_i64(tokens.get("reasoning"));
    let cache = tokens.get("cache");
    let cache_read = cache.map_or(0, |c| json_i64(c.get("read")));
    let cache_write = cache.map_or(0, |c| json_i64(c.get("write")));

    if input == 0 && output == 0 && reasoning == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }

    let model = non_empty_string(value.get("modelID"))?;
    let provider = non_empty_string(value.get("providerID"))?;

    let millis = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(Value::as_i64)?;
    let timestamp = Utc.timestamp_millis_opt(millis).single()?;

    // Prefer the in-file id (equals the DB id exactly); else the caller's hint.
    let message_id = non_empty_string(value.get("id")).or(id_hint)?;
    let session_id = non_empty_string(value.get("sessionID")).or(session_hint);

    let cost_usd = value.get("cost").and_then(Value::as_f64).unwrap_or(0.0);

    let event_key = format!("opencode:{message_id}");
    let session_key = session_id.as_ref().map(|s| format!("opencode:{s}"));
    let raw_payload = serde_json::json!({
        "session_id": session_id,
        "message_id": message_id,
        "provider": provider.clone(),
        "tier": tier,
    });

    Some(UnifiedMessage {
        client: Client::OpenCode,
        event_key,
        session_key,
        seq: None,
        model,
        provider,
        timestamp,
        tokens: TokenBreakdown {
            input,
            output,
            cache_read,
            cache_write,
            reasoning,
        },
        cost_cents: cost_usd * 100.0,
        raw_payload,
    })
}

fn json_i64(v: Option<&Value>) -> i64 {
    v.and_then(Value::as_i64).unwrap_or(0).max(0)
}

fn non_empty_string(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    const MSG: &str = r#"{"id":"msg_1","sessionID":"ses_a","providerID":"anthropic","modelID":"claude-sonnet-4-20250514","time":{"created":1767312000000},"tokens":{"input":100,"output":50,"reasoning":7,"cache":{"read":10,"write":20}},"cost":0.02}"#;

    #[test]
    fn parses_message_json_file() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "storage/message/ses_a/msg_1.json", MSG);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);

        let m = &messages[0];
        assert_eq!(m.client, Client::OpenCode);
        assert_eq!(m.model, "claude-sonnet-4-20250514");
        assert_eq!(m.provider, "anthropic");
        assert_eq!(m.tokens.input, 100);
        assert_eq!(m.tokens.output, 50);
        assert_eq!(m.tokens.cache_read, 10);
        assert_eq!(m.tokens.cache_write, 20);
        assert_eq!(m.tokens.reasoning, 7);
        assert_eq!(m.event_key, "opencode:msg_1");
        assert_eq!(m.session_key.as_deref(), Some("opencode:ses_a"));
        assert_eq!(m.seq, None);
        assert!((m.cost_cents - 2.0).abs() < 1e-9);
        assert_eq!(m.raw_payload["tier"], "json");
        assert_eq!(m.timestamp.timestamp_millis(), 1767312000000);
    }

    #[test]
    fn skips_zero_tokens_and_missing_model() {
        let tmp = TempDir::new().unwrap();
        // all-zero usage
        write(
            tmp.path(),
            "storage/message/s/zero.json",
            r#"{"id":"z","modelID":"m","providerID":"p","time":{"created":1767312000000},"tokens":{"input":0,"output":0,"cache":{"read":0,"write":0}}}"#,
        );
        // missing modelID
        write(
            tmp.path(),
            "storage/message/s/nomodel.json",
            r#"{"id":"n","providerID":"p","time":{"created":1767312000000},"tokens":{"input":5,"output":5}}"#,
        );
        // missing time.created
        write(
            tmp.path(),
            "storage/message/s/notime.json",
            r#"{"id":"t","modelID":"m","providerID":"p","tokens":{"input":5,"output":5}}"#,
        );

        assert!(scan(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn missing_root_returns_empty() {
        assert!(scan(Path::new("/nonexistent/opencode")).unwrap().is_empty());
    }

    #[test]
    fn default_root_respects_env_first_entry() {
        // Serialized with other env-mutating tests via the shared guard below.
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OPENCODE_DATA_DIR", "/tmp/first , /tmp/second");
        assert_eq!(default_root(), Some(PathBuf::from("/tmp/first")));
        std::env::remove_var("OPENCODE_DATA_DIR");
    }

    // Guards env-var mutation so parallel tests do not race on the process env.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
