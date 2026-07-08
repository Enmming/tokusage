//! Claude Code session parser.
//!
//! Reads JSONL files under `~/.claude/projects/**/*.jsonl`. Each line is an
//! entry; only `type=assistant` entries carry `message.usage` which is what
//! we care about.

use super::ScanResult;
use crate::model::{Client, TokenBreakdown, UnifiedMessage};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn default_root() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".claude/projects"))
}

#[derive(Debug, Deserialize)]
struct ClaudeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    uuid: Option<String>,
    timestamp: Option<String>,
    message: Option<ClaudeMessage>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
}

/// Walk `root`, parse every `*.jsonl`, collect one UnifiedMessage per
/// assistant entry with a usage block. Silently skips malformed lines — Claude
/// Code transcripts mix many event types and we only want the ones with usage.
pub fn scan(root: &Path) -> ScanResult {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut messages = Vec::new();
    let mut seen_event_keys = HashSet::new();

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        match parse_file(root, path) {
            Ok(file_messages) => {
                for message in file_messages {
                    if seen_event_keys.insert(message.event_key.clone()) {
                        messages.push(message);
                    }
                }
            }
            Err(err) => {
                tracing::warn!(?path, error = %err, "failed to parse Claude JSONL");
            }
        }
    }

    Ok(messages)
}

fn parse_file(root: &Path, path: &Path) -> anyhow::Result<Vec<UnifiedMessage>> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let relative_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut event_index: HashMap<String, usize> = HashMap::new();
    let mut file_messages = Vec::new();

    for (line_index, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        let entry: ClaudeEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if let Some(parsed) = to_unified(entry, &relative_path, line_index + 1) {
            if let Some(existing_index) = event_index.get(&parsed.logical_key).copied() {
                // Claude JSONL often repeats the same logical request/message while
                // streaming. Keep only the latest snapshot so submit emits one raw
                // event per logical Claude response.
                file_messages[existing_index] = parsed.message;
            } else {
                event_index.insert(parsed.logical_key, file_messages.len());
                file_messages.push(parsed.message);
            }
        }
    }

    Ok(file_messages)
}

struct ParsedClaudeMessage {
    logical_key: String,
    message: UnifiedMessage,
}

fn to_unified(
    entry: ClaudeEntry,
    relative_path: &str,
    line_number: usize,
) -> Option<ParsedClaudeMessage> {
    if entry.entry_type != "assistant" {
        return None;
    }

    let uuid = entry.uuid?;
    let message = entry.message?;
    let usage = message.usage?;
    let model = message.model?;
    let msg_id = message.id?;
    let request_id = entry.request_id;
    let ts_str = entry.timestamp?;
    let timestamp = parse_timestamp(&ts_str)?;
    let logical_key = match request_id.as_deref() {
        Some(request_id) => format!("claude:{}:{}", request_id, msg_id),
        None => format!("claude:{}:{}", relative_path, msg_id),
    };

    Some(ParsedClaudeMessage {
        logical_key,
        message: UnifiedMessage {
            client: Client::Claude,
            event_key: format!("claude:{}", uuid),
            session_key: Some(format!(
                "claude:sha256:{}",
                sha256_hex(relative_path.as_bytes())
            )),
            seq: Some(line_number as u64),
            model,
            provider: "anthropic".to_string(),
            timestamp,
            tokens: TokenBreakdown {
                input: usage.input_tokens,
                output: usage.output_tokens,
                cache_read: usage.cache_read_input_tokens,
                cache_write: usage.cache_creation_input_tokens,
                reasoning: 0,
            },
            cost_cents: 0.0, // Priced server-side or by a future pricing module.
            raw_payload: serde_json::json!({
                "request_id": request_id,
                "message_id": msg_id,
                "uuid": uuid,
            }),
        },
    })
}

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
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

    fn write_jsonl(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn parses_assistant_entries_with_usage() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"type":"permission-mode","permissionMode":"default"}
{"type":"user","timestamp":"2026-04-16T16:17:30Z","message":{"content":"hi"}}
{"type":"assistant","uuid":"uuid-1","timestamp":"2026-04-16T16:17:41.228Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":6,"output_tokens":197,"cache_read_input_tokens":16757,"cache_creation_input_tokens":10792}}}
{"type":"assistant","uuid":"uuid-2","timestamp":"2026-04-16T16:18:00Z","requestId":"req_B","message":{"id":"msg_2","model":"claude-sonnet-4-6","usage":{"input_tokens":10,"output_tokens":50}}}
"#;
        write_jsonl(tmp.path(), "session.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 2);

        let first = &messages[0];
        assert_eq!(first.client, Client::Claude);
        assert_eq!(first.model, "claude-opus-4-7");
        assert_eq!(first.tokens.input, 6);
        assert_eq!(first.tokens.output, 197);
        assert_eq!(first.tokens.cache_read, 16757);
        assert_eq!(first.tokens.cache_write, 10792);
        assert_eq!(first.event_key, "claude:uuid-1");
        assert_eq!(
            first.session_key.as_deref(),
            Some("claude:sha256:fc378a709b7d6f3aad1c8d1cc459e1b10ba6685b2ea5a7fe7a143d95fa6f4237")
        );
        assert_eq!(first.seq, Some(3));
        assert_eq!(first.raw_payload["request_id"], "req_A");
        assert_eq!(first.raw_payload["message_id"], "msg_1");
        assert_eq!(first.raw_payload["uuid"], "uuid-1");

        let second = &messages[1];
        assert_eq!(second.tokens.cache_read, 0);
        assert_eq!(second.event_key, "claude:uuid-2");
        assert_eq!(second.seq, Some(4));
    }

    #[test]
    fn skips_non_assistant_and_missing_usage() {
        let tmp = TempDir::new().unwrap();
        // user entries, permission-mode, assistant without usage, and entries
        // without a stable uuid must all be skipped.
        let jsonl = r#"{"type":"user","timestamp":"2026-04-16T16:17:30Z"}
{"type":"assistant","timestamp":"2026-04-16T16:17:41Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7"}}
{"type":"assistant","timestamp":"2026-04-16T16:18:00Z","message":{"id":"msg_2","model":"claude-sonnet-4-6","usage":{"input_tokens":1,"output_tokens":1}}}
"#;
        write_jsonl(tmp.path(), "x.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 0);
    }

    #[test]
    fn parses_external_api_entries_without_request_id() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"type":"assistant","uuid":"uuid-external-1","timestamp":"2026-04-16T16:17:41Z","userType":"external","sessionId":"session-1","message":{"id":"msg_external_1","model":"claude-sonnet-4-20250514","usage":{"input_tokens":8,"output_tokens":13,"cache_read_input_tokens":21,"cache_creation_input_tokens":34}}}"#;
        write_jsonl(tmp.path(), "-C-Users-gd-project/session.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].event_key, "claude:uuid-external-1");
        assert_eq!(messages[0].model, "claude-sonnet-4-20250514");
        assert_eq!(messages[0].tokens.input, 8);
        assert_eq!(messages[0].tokens.output, 13);
        assert_eq!(messages[0].tokens.cache_read, 21);
        assert_eq!(messages[0].tokens.cache_write, 34);
        assert_eq!(
            messages[0].raw_payload["request_id"],
            serde_json::Value::Null
        );
        assert_eq!(messages[0].raw_payload["message_id"], "msg_external_1");
    }

    #[test]
    fn walks_nested_directories() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"type":"assistant","uuid":"uuid-1","timestamp":"2026-04-16T16:17:41Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        write_jsonl(tmp.path(), "-Users-foo/session1.jsonl", jsonl);
        write_jsonl(
            tmp.path(),
            "-Users-foo/0abc/subagents/agent-xxx.jsonl",
            jsonl,
        );

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn missing_root_returns_empty() {
        let path = Path::new("/nonexistent/path/to/claude/projects");
        let messages = scan(path).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"not json at all
{"type":"assistant","uuid":"uuid-1","timestamp":"2026-04-16T16:17:41Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}
{broken"#;
        write_jsonl(tmp.path(), "x.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn keeps_latest_snapshot_for_repeated_logical_event() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"type":"assistant","uuid":"uuid-1","timestamp":"2026-04-16T16:17:41Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}
{"type":"assistant","uuid":"uuid-2","timestamp":"2026-04-16T16:17:42Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":99}}}
"#;
        write_jsonl(tmp.path(), "session.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].event_key, "claude:uuid-2");
        assert_eq!(messages[0].tokens.output, 99);
        assert_eq!(messages[0].seq, Some(2));
        assert_eq!(
            messages[0].timestamp.to_rfc3339(),
            "2026-04-16T16:17:42+00:00"
        );
        assert_eq!(messages[0].raw_payload["uuid"], "uuid-2");
    }
}
