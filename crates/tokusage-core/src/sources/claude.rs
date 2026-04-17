//! Claude Code session parser.
//!
//! Reads JSONL files under `~/.claude/projects/**/*.jsonl`. Each line is an
//! entry; only `type=assistant` entries carry `message.usage` which is what
//! we care about.

use super::ScanResult;
use crate::model::{Client, TokenBreakdown, UnifiedMessage};
use chrono::{DateTime, Utc};
use serde::Deserialize;
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

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        if let Err(err) = parse_file_into(path, &mut messages) {
            tracing::warn!(?path, error = %err, "failed to parse Claude JSONL");
        }
    }

    Ok(messages)
}

fn parse_file_into(path: &Path, out: &mut Vec<UnifiedMessage>) -> anyhow::Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        let entry: ClaudeEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if let Some(msg) = to_unified(entry) {
            out.push(msg);
        }
    }

    Ok(())
}

fn to_unified(entry: ClaudeEntry) -> Option<UnifiedMessage> {
    if entry.entry_type != "assistant" {
        return None;
    }

    let message = entry.message?;
    let usage = message.usage?;
    let model = message.model?;
    let msg_id = message.id?;
    let request_id = entry.request_id?;
    let ts_str = entry.timestamp?;
    let timestamp = parse_timestamp(&ts_str)?;

    Some(UnifiedMessage {
        client: Client::Claude,
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
        dedup_key: format!("claude:{}:{}", request_id, msg_id),
    })
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
{"type":"assistant","timestamp":"2026-04-16T16:17:41.228Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":6,"output_tokens":197,"cache_read_input_tokens":16757,"cache_creation_input_tokens":10792}}}
{"type":"assistant","timestamp":"2026-04-16T16:18:00Z","requestId":"req_B","message":{"id":"msg_2","model":"claude-sonnet-4-6","usage":{"input_tokens":10,"output_tokens":50}}}
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
        assert_eq!(first.dedup_key, "claude:req_A:msg_1");

        let second = &messages[1];
        assert_eq!(second.tokens.cache_read, 0);
        assert_eq!(second.dedup_key, "claude:req_B:msg_2");
    }

    #[test]
    fn skips_non_assistant_and_missing_usage() {
        let tmp = TempDir::new().unwrap();
        // user entries, permission-mode, and assistant without usage/requestId must all be skipped.
        let jsonl = r#"{"type":"user","timestamp":"2026-04-16T16:17:30Z"}
{"type":"assistant","timestamp":"2026-04-16T16:17:41Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7"}}
{"type":"assistant","timestamp":"2026-04-16T16:18:00Z","message":{"id":"msg_2","model":"claude-sonnet-4-6","usage":{"input_tokens":1,"output_tokens":1}}}
"#;
        write_jsonl(tmp.path(), "x.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 0);
    }

    #[test]
    fn walks_nested_directories() {
        let tmp = TempDir::new().unwrap();
        let jsonl = r#"{"type":"assistant","timestamp":"2026-04-16T16:17:41Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        write_jsonl(tmp.path(), "-Users-foo/session1.jsonl", jsonl);
        write_jsonl(
            tmp.path(),
            "-Users-foo/0abc/subagents/agent-xxx.jsonl",
            jsonl,
        );

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 2);
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
{"type":"assistant","timestamp":"2026-04-16T16:17:41Z","requestId":"req_A","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}
{broken"#;
        write_jsonl(tmp.path(), "x.jsonl", jsonl);

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);
    }
}
