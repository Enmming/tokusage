//! Codex CLI session parser.
//!
//! Reads JSONL files under `$CODEX_HOME/sessions/**/*.jsonl` (default
//! `~/.codex/sessions`). Each session file has a predictable prefix:
//!
//! 1. `session_meta` (first line) — gives `session.id` and `model_provider`.
//! 2. Alternating `turn_context` (declares current model) and
//!    `event_msg/token_count` (reports this turn's usage).
//!
//! Codex already emits **per-turn** deltas under
//! `payload.info.last_token_usage`, so we consume those directly rather than
//! computing deltas off the running totals.

use super::ScanResult;
use crate::model::{Client, TokenBreakdown, UnifiedMessage};
use chrono::{DateTime, Utc};
use serde::Deserialize;
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
    let mut turn_index: u32 = 0;

    for line in reader.lines() {
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
                }
            }
            "event_msg" => {
                let payload_type = payload
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if payload_type != "token_count" {
                    continue;
                }
                let parsed: TokenCountPayload =
                    match serde_json::from_value(payload.clone()) {
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
                let Some(ts_str) = entry.timestamp.as_deref() else {
                    continue;
                };
                let Some(timestamp) = parse_timestamp(ts_str) else {
                    continue;
                };

                out.push(UnifiedMessage {
                    client: Client::Codex,
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
                    dedup_key: format!("codex:{}:{}", sid, turn_index),
                });
                turn_index += 1;
            }
            _ => {}
        }
    }

    Ok(())
}

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
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
{"timestamp":"2026-03-19T16:28:46.076Z","type":"turn_context","payload":{"turn_id":"t2","model":"gpt-5.4"}}
{"timestamp":"2026-03-19T16:28:46.076Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":12974,"cached_input_tokens":0,"output_tokens":349,"reasoning_output_tokens":100,"total_tokens":13423}}}}
"#;
        write(tmp.path(), "2026/03/19/s.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 2);

        let first = &messages[0];
        assert_eq!(first.client, Client::Codex);
        assert_eq!(first.model, "gpt-5.4");
        assert_eq!(first.provider, "openai");
        assert_eq!(first.tokens.input, 11692 - 9600); // input minus cached
        assert_eq!(first.tokens.output, 427);
        assert_eq!(first.tokens.cache_read, 9600);
        assert_eq!(first.tokens.reasoning, 206);
        assert_eq!(first.dedup_key, "codex:sess-abc:0");

        let second = &messages[1];
        assert_eq!(second.tokens.input, 12974);
        assert_eq!(second.tokens.cache_read, 0);
        assert_eq!(second.dedup_key, "codex:sess-abc:1");
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
    fn missing_root_returns_empty() {
        let path = Path::new("/nonexistent/codex");
        assert!(scan(path).unwrap().is_empty());
    }
}
