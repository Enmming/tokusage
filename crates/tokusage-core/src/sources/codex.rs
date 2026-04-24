//! Codex CLI session parser.
//!
//! Reads JSONL files under `$CODEX_HOME/sessions/**/*.jsonl` (default
//! `~/.codex/sessions`). Each session file has a predictable prefix:
//!
//! 1. `session_meta` (first line) — gives `session.id` and `model_provider`.
//! 2. Alternating `turn_context` (declares current model) and
//!    `event_msg/token_count` (reports incremental deltas within that turn).
//!
//! `payload.info.last_token_usage` is the per-snapshot delta we want to keep.
//! Some sessions emit identical same-timestamp twins that differ only by rate
//! limit metadata; we collapse those into one event by fingerprinting the usage
//! payload itself.

use super::ScanResult;
use crate::model::{Client, TokenBreakdown, UnifiedMessage};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn default_root() -> Option<PathBuf> {
    if let Ok(var) = std::env::var("CODEX_HOME") {
        return Some(PathBuf::from(var).join("sessions"));
    }
    directories::BaseDirs::new().map(|d| d.home_dir().join(".codex/sessions"))
}

#[derive(Debug, Deserialize)]
struct CodexEntry {
    #[serde(rename = "type")]
    entry_type: String,
    timestamp: Option<String>,
    payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct SessionMetaPayload {
    id: Option<String>,
    model_provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TurnContextPayload {
    model: Option<String>,
    turn_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenCountPayload {
    info: Option<TokenCountInfo>,
}

#[derive(Debug, Deserialize)]
struct TokenCountInfo {
    last_token_usage: Option<CodexUsage>,
}

#[derive(Debug, Deserialize, Default)]
struct CodexUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cached_input_tokens: i64,
    #[serde(default)]
    reasoning_output_tokens: i64,
}

pub fn scan(root: &Path) -> ScanResult {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut messages = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        if let Err(err) = parse_session_into(path, &mut messages) {
            tracing::warn!(?path, error = %err, "failed to parse Codex session");
        }
    }

    Ok(messages)
}

fn parse_session_into(path: &Path, out: &mut Vec<UnifiedMessage>) -> anyhow::Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut session_id: Option<String> = None;
    let mut provider: String = "openai".to_string();
    let mut current_model: Option<String> = None;
    let mut current_turn_id: Option<String> = None;
    let mut current_turn_index: Option<u32> = None;
    let mut next_turn_index: u32 = 0;
    let mut current_snapshot_index: u64 = 0;
    let mut current_turn_events: HashMap<String, (usize, u64)> = HashMap::new();
    let mut file_messages = Vec::new();

    for (line_number, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        let entry: CodexEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let payload = match entry.payload.as_ref() {
            Some(p) => p,
            None => continue,
        };

        match entry.entry_type.as_str() {
            "session_meta" => {
                if let Ok(p) = serde_json::from_value::<SessionMetaPayload>(payload.clone()) {
                    session_id = p.id;
                    if let Some(mp) = p.model_provider {
                        provider = mp;
                    }
                }
            }
            "turn_context" => {
                if let Ok(p) = serde_json::from_value::<TurnContextPayload>(payload.clone()) {
                    if let Some(model) = p.model {
                        current_model = Some(model);
                    }
                    current_turn_id = p.turn_id;
                    current_turn_index = Some(next_turn_index);
                    next_turn_index += 1;
                    current_snapshot_index = 0;
                    current_turn_events.clear();
                }
            }
            "event_msg" => {
                let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if payload_type != "token_count" {
                    continue;
                }
                let parsed: TokenCountPayload = match serde_json::from_value(payload.clone()) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let info = match parsed.info {
                    Some(i) => i,
                    None => continue,
                };
                let usage = match info.last_token_usage {
                    Some(u) => u,
                    None => continue,
                };
                if usage.input_tokens == 0
                    && usage.output_tokens == 0
                    && usage.cached_input_tokens == 0
                    && usage.reasoning_output_tokens == 0
                {
                    // Skip empty delta entries (can happen at session start).
                    continue;
                }

                let Some(sid) = session_id.as_deref() else {
                    continue;
                };
                let Some(model) = current_model.as_deref() else {
                    continue;
                };
                let Some(turn_index) = current_turn_index else {
                    continue;
                };
                let Some(ts_str) = entry.timestamp.as_deref() else {
                    continue;
                };
                let Some(timestamp) = parse_timestamp(ts_str) else {
                    continue;
                };
                let turn_label = current_turn_id
                    .clone()
                    .unwrap_or_else(|| format!("turn-{}", turn_index));
                let usage_signature = usage_signature(&usage);
                let event_key = format!(
                    "codex:{}:{}:{}:{}",
                    sid, turn_label, ts_str, usage_signature
                );
                let session_key = format!("codex:{}:{}", sid, turn_label);
                let rate_limit_id = payload
                    .get("rate_limits")
                    .and_then(|value| value.get("limit_id"))
                    .and_then(|value| value.as_str());
                let rate_limit_name = payload
                    .get("rate_limits")
                    .and_then(|value| value.get("limit_name"))
                    .and_then(|value| value.as_str());

                if let Some((existing_index, snapshot_index)) =
                    current_turn_events.get(&event_key).copied()
                {
                    file_messages[existing_index] = UnifiedMessage {
                        client: Client::Codex,
                        event_key,
                        session_key: Some(session_key),
                        seq: Some(snapshot_index),
                        model: model.to_string(),
                        provider: provider.clone(),
                        timestamp,
                        tokens: TokenBreakdown {
                            // Codex reports cached separately; we treat cached_input as cache_read
                            // and the remaining input_tokens - cached_input_tokens as fresh input.
                            input: (usage.input_tokens - usage.cached_input_tokens).max(0),
                            output: usage.output_tokens.max(0),
                            cache_read: usage.cached_input_tokens.max(0),
                            cache_write: 0,
                            reasoning: usage.reasoning_output_tokens.max(0),
                        },
                        cost_cents: 0.0,
                        raw_payload: serde_json::json!({
                            "session_id": sid,
                            "turn_id": current_turn_id,
                            "turn_index": turn_index,
                            "snapshot_index": snapshot_index,
                            "line_number": line_number + 1,
                            "rate_limit_id": rate_limit_id,
                            "rate_limit_name": rate_limit_name,
                        }),
                    };
                    continue;
                }

                let snapshot_index = current_snapshot_index;
                current_snapshot_index += 1;
                current_turn_events
                    .insert(event_key.clone(), (file_messages.len(), snapshot_index));
                file_messages.push(UnifiedMessage {
                    client: Client::Codex,
                    event_key,
                    session_key: Some(session_key),
                    seq: Some(snapshot_index),
                    model: model.to_string(),
                    provider: provider.clone(),
                    timestamp,
                    tokens: TokenBreakdown {
                        // Codex reports cached separately; we treat cached_input as cache_read
                        // and the remaining input_tokens - cached_input_tokens as fresh input.
                        input: (usage.input_tokens - usage.cached_input_tokens).max(0),
                        output: usage.output_tokens.max(0),
                        cache_read: usage.cached_input_tokens.max(0),
                        cache_write: 0,
                        reasoning: usage.reasoning_output_tokens.max(0),
                    },
                    cost_cents: 0.0,
                    raw_payload: serde_json::json!({
                        "session_id": sid,
                        "turn_id": current_turn_id,
                        "turn_index": turn_index,
                        "snapshot_index": snapshot_index,
                        "line_number": line_number + 1,
                        "rate_limit_id": rate_limit_id,
                        "rate_limit_name": rate_limit_name,
                    }),
                });
            }
            _ => {}
        }
    }

    out.extend(file_messages);
    Ok(())
}

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn usage_signature(usage: &CodexUsage) -> String {
    let payload = format!(
        "{}:{}:{}:{}",
        usage.input_tokens,
        usage.cached_input_tokens,
        usage.output_tokens,
        usage.reasoning_output_tokens
    );
    sha256_hex(payload.as_bytes())
}

fn sha256_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
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

    #[test]
    fn parses_real_codex_schema() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"timestamp":"2026-03-19T16:28:24.244Z","type":"session_meta","payload":{"id":"sess-abc","model_provider":"openai"}}
{"timestamp":"2026-03-19T16:28:24.245Z","type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.4"}}
{"timestamp":"2026-03-19T16:28:33.304Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":11692,"cached_input_tokens":9600,"output_tokens":427,"reasoning_output_tokens":206,"total_tokens":12119},"total_token_usage":{"input_tokens":11692,"cached_input_tokens":9600,"output_tokens":427,"reasoning_output_tokens":206,"total_tokens":12119}}}}
{"timestamp":"2026-03-19T16:28:46.076Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":2000,"cached_input_tokens":1000,"output_tokens":10,"reasoning_output_tokens":5,"total_tokens":2015},"total_token_usage":{"input_tokens":13692,"cached_input_tokens":10600,"output_tokens":437,"reasoning_output_tokens":211,"total_tokens":14140}}}}
{"timestamp":"2026-03-19T16:28:46.076Z","type":"turn_context","payload":{"turn_id":"t2","model":"gpt-5.4"}}
{"timestamp":"2026-03-19T16:28:46.076Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":12974,"cached_input_tokens":0,"output_tokens":349,"reasoning_output_tokens":100,"total_tokens":13423}}}}
"#;
        write(tmp.path(), "2026/03/19/s.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 3);

        let first = &messages[0];
        assert_eq!(first.client, Client::Codex);
        assert_eq!(first.model, "gpt-5.4");
        assert_eq!(first.provider, "openai");
        assert_eq!(first.tokens.input, 11692 - 9600); // input minus cached
        assert_eq!(first.tokens.output, 427);
        assert_eq!(first.tokens.cache_read, 9600);
        assert_eq!(first.tokens.reasoning, 206);
        assert!(first
            .event_key
            .starts_with("codex:sess-abc:t1:2026-03-19T16:28:33.304Z:"));
        assert_eq!(first.session_key.as_deref(), Some("codex:sess-abc:t1"));
        assert_eq!(first.seq, Some(0));
        assert_eq!(first.raw_payload["session_id"], "sess-abc");
        assert_eq!(first.raw_payload["turn_id"], "t1");
        assert_eq!(first.raw_payload["turn_index"], 0);
        assert_eq!(first.raw_payload["snapshot_index"], 0);

        let second = &messages[1];
        assert_eq!(second.tokens.input, 1000);
        assert_eq!(second.tokens.output, 10);
        assert_eq!(second.tokens.cache_read, 1000);
        assert_eq!(second.tokens.reasoning, 5);
        assert!(second
            .event_key
            .starts_with("codex:sess-abc:t1:2026-03-19T16:28:46.076Z:"));
        assert_eq!(second.session_key.as_deref(), Some("codex:sess-abc:t1"));
        assert_eq!(second.seq, Some(1));
        assert_eq!(second.raw_payload["snapshot_index"], 1);

        let third = &messages[2];
        assert_eq!(third.tokens.input, 12974);
        assert_eq!(third.tokens.cache_read, 0);
        assert!(third
            .event_key
            .starts_with("codex:sess-abc:t2:2026-03-19T16:28:46.076Z:"));
        assert_eq!(third.session_key.as_deref(), Some("codex:sess-abc:t2"));
        assert_eq!(third.seq, Some(0));
    }

    #[test]
    fn falls_back_when_turn_id_missing() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"timestamp":"2026-03-19T16:28:24.244Z","type":"session_meta","payload":{"id":"sess-abc","model_provider":"openai"}}
{"timestamp":"2026-03-19T16:28:24.245Z","type":"turn_context","payload":{"model":"gpt-5.4"}}
{"timestamp":"2026-03-19T16:28:33.304Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":4}}}}
"#;
        write(tmp.path(), "2026/03/19/s.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);

        let first = &messages[0];
        assert!(first
            .event_key
            .starts_with("codex:sess-abc:turn-0:2026-03-19T16:28:33.304Z:"));
        assert_eq!(first.session_key.as_deref(), Some("codex:sess-abc:turn-0"));
        assert_eq!(first.seq, Some(0));
        assert_eq!(first.raw_payload["session_id"], "sess-abc");
        assert!(first.raw_payload["turn_id"].is_null());
        assert_eq!(first.raw_payload["turn_index"], 0);
        assert_eq!(first.raw_payload["snapshot_index"], 0);
    }

    #[test]
    fn skips_event_msg_other_than_token_count() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"timestamp":"2026-03-19T16:28:24Z","type":"session_meta","payload":{"id":"s","model_provider":"openai"}}
{"timestamp":"2026-03-19T16:28:24Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-03-19T16:28:33Z","type":"event_msg","payload":{"type":"agent_message","text":"hello"}}
{"timestamp":"2026-03-19T16:28:34Z","type":"event_msg","payload":{"type":"task_started"}}
"#;
        write(tmp.path(), "x.jsonl", jsonl);
        let m = scan(tmp.path()).unwrap();
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn empty_delta_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"timestamp":"2026-03-19T16:28:24Z","type":"session_meta","payload":{"id":"s","model_provider":"openai"}}
{"timestamp":"2026-03-19T16:28:24Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-03-19T16:28:33Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"output_tokens":0,"cached_input_tokens":0,"reasoning_output_tokens":0}}}}
"#;
        write(tmp.path(), "x.jsonl", jsonl);
        let m = scan(tmp.path()).unwrap();
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn model_change_mid_session_uses_latest() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"timestamp":"2026-03-19T16:28:24Z","type":"session_meta","payload":{"id":"s","model_provider":"openai"}}
{"timestamp":"2026-03-19T16:28:24Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-03-19T16:28:33Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5}}}}
{"timestamp":"2026-03-19T16:29:00Z","type":"turn_context","payload":{"model":"gpt-5.4"}}
{"timestamp":"2026-03-19T16:29:10Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"output_tokens":10}}}}
"#;
        write(tmp.path(), "x.jsonl", jsonl);
        let m = scan(tmp.path()).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].model, "gpt-5");
        assert_eq!(m[1].model, "gpt-5.4");
    }

    #[test]
    fn collapses_same_timestamp_usage_twins() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"timestamp":"2026-04-23T05:50:27.989Z","type":"session_meta","payload":{"id":"sess-abc","model_provider":"openai"}}
{"timestamp":"2026-04-23T05:50:27.989Z","type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.4"}}
{"timestamp":"2026-04-23T05:50:27.989Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":55989,"cached_input_tokens":54912,"output_tokens":925,"reasoning_output_tokens":866,"total_tokens":56914}},"rate_limits":{"limit_id":"codex","limit_name":null}}}
{"timestamp":"2026-04-23T05:50:27.989Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":55989,"cached_input_tokens":54912,"output_tokens":925,"reasoning_output_tokens":866,"total_tokens":56914}},"rate_limits":{"limit_id":"codex_bengalfox","limit_name":"GPT-5.3-Codex-Spark"}}}
{"timestamp":"2026-04-23T05:51:49.576Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":61536,"cached_input_tokens":60672,"output_tokens":188,"reasoning_output_tokens":115,"total_tokens":61724}},"rate_limits":{"limit_id":"codex_bengalfox","limit_name":"GPT-5.3-Codex-Spark"}}}
"#;
        write(tmp.path(), "2026/04/23/s.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].seq, Some(0));
        assert_eq!(messages[1].seq, Some(1));
        assert_eq!(messages[0].raw_payload["rate_limit_id"], "codex_bengalfox");
        assert_eq!(messages[0].raw_payload["line_number"], serde_json::json!(4));
    }

    #[test]
    fn missing_root_returns_empty() {
        let path = Path::new("/nonexistent/codex");
        assert!(scan(path).unwrap().is_empty());
    }
}
