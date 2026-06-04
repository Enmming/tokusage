# OpenCode Usage Source Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add [opencode](https://opencode.ai/) as a fourth local usage source so `tokusage show` and `tokusage submit` report its token usage from local files.

**Architecture:** A new `sources/opencode.rs` module mirrors `sources/codex.rs`: it parses opencode's two storage tiers (legacy `storage/message/**/*.json` files and the newer `opencode.db` SQLite, deduped by message id) into `UnifiedMessage`s. The new `Client::OpenCode` variant flows through `collect.rs` and renders in `show.rs`.

**Tech Stack:** Rust workspace (`tokusage-core` + `tokusage-cli`). Existing deps reused: `serde_json`, `walkdir`, `directories`, `rusqlite` (read-only), `chrono`, `tracing`; `tempfile` for tests. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-06-02-opencode-usage-source-design.md`

---

## File Structure

| File | Responsibility | Change |
|------|----------------|--------|
| `crates/tokusage-core/src/model.rs` | `Client` enum + `as_str` | Add `OpenCode` variant + arm + unit test |
| `crates/tokusage-core/src/sources/opencode.rs` | opencode parser (JSON + SQLite, dedup) | **Create** |
| `crates/tokusage-core/src/sources/mod.rs` | source module registry | Add `pub mod opencode;` |
| `crates/tokusage-cli/src/commands/show.rs` | chart rendering + aggregation | Add to `order`, `client_name`, widen column, update tests |
| `crates/tokusage-cli/src/main.rs` | `SourceArg` + CLI help | Add `OpenCode`, update `--source` help + `about` |
| `crates/tokusage-cli/src/collect.rs` | source fan-out | Add `collect_opencode()` + wire both arms |
| `README.md` | docs | Add OpenCode to sources + table + prose |

**Build-green ordering:** Adding `Client::OpenCode` makes the exhaustive matches in `model.rs::as_str` **and** `show.rs::client_name` non-exhaustive, so both are fixed in Task 1. Adding `SourceArg::OpenCode` makes the exhaustive match in `collect.rs` non-exhaustive, so that is fixed in Task 4.

---

## Task 1: Wire the `OpenCode` client through model + render

Adds the enum variant and every exhaustive-match arm + the render touch-ups, so the workspace still compiles and `tokusage show` displays an (empty) OpenCode row.

**Files:**
- Modify: `crates/tokusage-core/src/model.rs`
- Modify: `crates/tokusage-cli/src/commands/show.rs`

- [ ] **Step 1: Add the failing model test**

In `crates/tokusage-core/src/model.rs`, append at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_client_is_lowercase_on_wire() {
        assert_eq!(Client::OpenCode.as_str(), "opencode");
        assert_eq!(
            serde_json::to_string(&Client::OpenCode).unwrap(),
            "\"opencode\""
        );
        let back: Client = serde_json::from_str("\"opencode\"").unwrap();
        assert_eq!(back, Client::OpenCode);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p tokusage-core opencode_client_is_lowercase_on_wire`
Expected: compile error — `no variant named OpenCode found for enum Client`.

- [ ] **Step 3: Add the `OpenCode` variant + `as_str` arm**

In `crates/tokusage-core/src/model.rs`, change the enum:

```rust
pub enum Client {
    Claude,
    Codex,
    Cursor,
    OpenCode,
}
```

and add the arm in `as_str`:

```rust
            Client::Cursor => "cursor",
            Client::OpenCode => "opencode",
```

- [ ] **Step 4: Fix the `show.rs` exhaustive match + render width**

In `crates/tokusage-cli/src/commands/show.rs`:

Add `OpenCode` to the `order` array in `aggregate`:

```rust
    let order = [
        Client::Claude,
        Client::Codex,
        Client::Cursor,
        Client::OpenCode,
    ];
```

Add the arm in `client_name`:

```rust
        Client::Cursor => "Cursor",
        Client::OpenCode => "OpenCode",
```

Widen the client-name column in `render` (so the 8-char "OpenCode" label keeps the bars aligned) — change `{:<7}` to `{:<8}`:

```rust
        out.push_str(&format!(
            "{:<8} {} {:<wb$} {:>6}  {} {:<wb$} {:>6}\n",
            client_name(c.client),
```

- [ ] **Step 5: Run the model test to verify it passes**

Run: `cargo test -p tokusage-core opencode_client_is_lowercase_on_wire`
Expected: PASS.

- [ ] **Step 6: Update the show.rs tests to include OpenCode**

In `crates/tokusage-cli/src/commands/show.rs` tests:

In `aggregate_buckets_by_client_and_month`, add an OpenCode current-month message to the `messages` vec (after the Cursor entry):

```rust
            msg(
                Client::OpenCode,
                Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap(),
                40,
            ), // current
```

and assert its bucket before the daily-series asserts:

```rust
        let opencode = report
            .per_client
            .iter()
            .find(|c| c.client == Client::OpenCode)
            .unwrap();
        assert_eq!(opencode.current.total(), 40);
```

Update the daily-series total in the same test from `130` to `170` (100 Claude + 30 Cursor + 40 OpenCode):

```rust
        assert_eq!(report.daily_current.iter().sum::<i64>(), 170);
```

In `render_contains_key_lines`, add an OpenCode entry to `per_client` (after the Cursor `ClientMonths`):

```rust
                ClientMonths {
                    client: Client::OpenCode,
                    current: tb(150_000),
                    last: tb(100_000),
                },
```

and assert it renders, next to the other `contains` asserts:

```rust
        assert!(s.contains("OpenCode"));
```

- [ ] **Step 7: Run the show tests to verify they pass**

Run: `cargo test -p tokusage-cli aggregate_buckets_by_client_and_month && cargo test -p tokusage-cli render_contains_key_lines`
Expected: PASS for both.

- [ ] **Step 8: Verify the whole workspace builds + tests pass**

Run: `cargo test`
Expected: all tests pass, no compile errors.

- [ ] **Step 9: Commit**

```bash
git add crates/tokusage-core/src/model.rs crates/tokusage-cli/src/commands/show.rs
git commit -m "feat: add OpenCode client variant and render row"
```

---

## Task 2: opencode JSON-file parser

Creates `sources/opencode.rs` with `default_root`, the shared JSON→`UnifiedMessage` mapping, and a `scan` that reads only the legacy `storage/message/**/*.json` tier. SQLite is added in Task 3.

**Files:**
- Create: `crates/tokusage-core/src/sources/opencode.rs`
- Modify: `crates/tokusage-core/src/sources/mod.rs`

- [ ] **Step 1: Register the module**

In `crates/tokusage-core/src/sources/mod.rs`, add (keeping alphabetical-ish order):

```rust
pub mod claude;
pub mod codex;
pub mod cursor;
pub mod opencode;
```

- [ ] **Step 2: Create the module with the JSON tier + failing tests**

Create `crates/tokusage-core/src/sources/opencode.rs` with:

```rust
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
```

- [ ] **Step 3: Run the new tests to verify they pass**

Run: `cargo test -p tokusage-core sources::opencode`
Expected: 4 tests pass (`parses_message_json_file`, `skips_zero_tokens_and_missing_model`, `missing_root_returns_empty`, `default_root_respects_env_first_entry`).

- [ ] **Step 4: Commit**

```bash
git add crates/tokusage-core/src/sources/opencode.rs crates/tokusage-core/src/sources/mod.rs
git commit -m "feat: parse opencode JSON message files"
```

---

## Task 3: opencode SQLite tier + cross-tier dedup

Extends `scan` to also read `opencode.db` (or a channel `opencode-*.db`) read-only and dedupe DB+JSON by message id, DB-first.

**Files:**
- Modify: `crates/tokusage-core/src/sources/opencode.rs`

- [ ] **Step 1: Add failing SQLite + dedup tests**

In `crates/tokusage-core/src/sources/opencode.rs` test module, add a DB helper and three tests:

```rust
    fn make_db(path: &Path, rows: &[(&str, &str, &str)]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute(
            "CREATE TABLE message (id TEXT, session_id TEXT, data TEXT)",
            [],
        )
        .unwrap();
        for (id, session_id, data) in rows {
            conn.execute(
                "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, session_id, data],
            )
            .unwrap();
        }
    }

    #[test]
    fn parses_sqlite_message() {
        let tmp = TempDir::new().unwrap();
        make_db(
            &tmp.path().join("opencode.db"),
            &[("msg_db", "ses_db", r#"{"providerID":"anthropic","modelID":"claude-sonnet-4-20250514","time":{"created":1767312000000},"tokens":{"input":120,"output":60,"cache":{"read":12,"write":24}},"cost":0.03}"#)],
        );

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);
        let m = &messages[0];
        assert_eq!(m.event_key, "opencode:msg_db");
        assert_eq!(m.session_key.as_deref(), Some("opencode:ses_db"));
        assert_eq!(m.tokens.input, 120);
        assert_eq!(m.tokens.cache_write, 24);
        assert_eq!(m.raw_payload["tier"], "db");
    }

    #[test]
    fn dedupes_json_and_db_by_id() {
        let tmp = TempDir::new().unwrap();
        // Same id "msg_1" in both tiers; the DB copy must win.
        write(tmp.path(), "storage/message/ses_a/msg_1.json", MSG);
        make_db(
            &tmp.path().join("opencode.db"),
            &[("msg_1", "ses_a", r#"{"id":"msg_1","providerID":"anthropic","modelID":"db-model","time":{"created":1767312000000},"tokens":{"input":1,"output":1}}"#)],
        );

        let messages = scan(tmp.path()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].model, "db-model"); // DB tier read first wins
        assert_eq!(messages[0].raw_payload["tier"], "db");
    }

    #[test]
    fn db_without_message_table_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path().join("opencode.db")).unwrap();
        conn.execute("CREATE TABLE other (x TEXT)", []).unwrap();
        drop(conn);

        assert!(scan(tmp.path()).unwrap().is_empty());
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p tokusage-core sources::opencode`
Expected: `parses_sqlite_message` and `dedupes_json_and_db_by_id` FAIL (DB rows are not read yet — 0 messages / JSON copy wins); `db_without_message_table_is_skipped` passes incidentally.

- [ ] **Step 3: Implement the DB tier**

In `crates/tokusage-core/src/sources/opencode.rs`, add `use std::fs;` to the imports, then insert the DB read **before** the JSON loop in `scan` (right after `let mut seen = ...`):

```rust
    // DB tier first (newer, migrated copy wins on a message-id collision).
    if let Some(db) = db_path(root) {
        match scan_db(&db) {
            Ok(msgs) => {
                for m in msgs {
                    if seen.insert(m.event_key.clone()) {
                        out.push(m);
                    }
                }
            }
            Err(err) => tracing::warn!(?db, error = %err, "failed to read opencode db"),
        }
    }
```

Then add these functions (after `scan`):

```rust
/// `opencode.db`, else the first `opencode-*.db` channel database, if present.
fn db_path(root: &Path) -> Option<PathBuf> {
    let default = root.join("opencode.db");
    if default.is_file() {
        return Some(default);
    }
    let mut channels: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.starts_with("opencode-") && n.ends_with(".db")
                })
        })
        .collect();
    channels.sort();
    channels.into_iter().next()
}

fn scan_db(db: &Path) -> anyhow::Result<Vec<UnifiedMessage>> {
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    // An older database may not have migrated to the `message` table yet.
    let mut stmt = match conn.prepare("SELECT id, session_id, data FROM message") {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let session_id: Option<String> = row.get(1)?;
        let data: String = row.get(2)?;
        Ok((id, session_id, data))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, session_id, data) = match row {
            Ok(v) => v,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(m) = message_value_to_unified(&value, Some(id), session_id, "db") {
            out.push(m);
        }
    }
    Ok(out)
}
```

- [ ] **Step 4: Run the opencode tests to verify they pass**

Run: `cargo test -p tokusage-core sources::opencode`
Expected: all 7 opencode tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-core/src/sources/opencode.rs
git commit -m "feat: read opencode SQLite tier and dedupe by message id"
```

---

## Task 4: Wire opencode into `collect` and the CLI

Adds `SourceArg::OpenCode`, the `collect_opencode()` fan-out, and CLI help/about text. Adding the `SourceArg` variant forces the `collect` match arm (compile gate).

**Files:**
- Modify: `crates/tokusage-cli/src/main.rs`
- Modify: `crates/tokusage-cli/src/collect.rs`

- [ ] **Step 1: Add the `OpenCode` SourceArg + help/about**

In `crates/tokusage-cli/src/main.rs`:

```rust
enum SourceArg {
    Claude,
    Codex,
    Cursor,
    OpenCode,
}
```

Update the per-arg help comment on the `Submit { source }` field:

```rust
        /// Only run a single source (claude|codex|cursor|opencode)
        #[arg(long)]
        source: Option<SourceArg>,
```

Update the top-level `about`:

```rust
    about = "Track AI coding tool token usage across Claude Code, Codex, Cursor, and OpenCode"
```

- [ ] **Step 2: Wire `collect_opencode` into both arms**

In `crates/tokusage-cli/src/collect.rs`, add the `Some` arm:

```rust
        Some(SourceArg::Cursor) => collect_cursor(),
        Some(SourceArg::OpenCode) => collect_opencode(),
```

add to the `None` aggregate (after the cursor block):

```rust
            match collect_opencode() {
                Ok(mut v) => out.append(&mut v),
                Err(e) => tracing::warn!("opencode source failed: {e}"),
            }
```

and add the helper:

```rust
fn collect_opencode() -> Result<Vec<UnifiedMessage>> {
    let root = sources::opencode::default_root()
        .context("could not resolve OpenCode data directory")?;
    sources::opencode::scan(&root)
}
```

- [ ] **Step 3: Verify the workspace builds + all tests pass**

Run: `cargo test`
Expected: all tests pass, no warnings about non-exhaustive matches.

- [ ] **Step 4: Manual end-to-end check against a temp data dir**

```bash
TMP=$(mktemp -d)
mkdir -p "$TMP/storage/message/ses_a"
cat > "$TMP/storage/message/ses_a/msg_1.json" <<'JSON'
{"id":"msg_1","sessionID":"ses_a","providerID":"anthropic","modelID":"claude-sonnet-4-20250514","time":{"created":1767312000000},"tokens":{"input":100,"output":50,"cache":{"read":10,"write":20}},"cost":0}
JSON
OPENCODE_DATA_DIR="$TMP" cargo run -q -p tokusage-cli -- show
OPENCODE_DATA_DIR="$TMP" cargo run -q -p tokusage-cli -- submit --source opencode --dry-run
```

Expected: `show` prints an `OpenCode` row with non-zero tokens for the month containing 2026-01-02 (UTC ts `1767312000000`); `submit --dry-run` prints a payload whose event has `"source":"opencode"`. (Adjust the `time.created` to the current month if you want it under the "this month" bar.)

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-cli/src/main.rs crates/tokusage-cli/src/collect.rs
git commit -m "feat: wire opencode source into collect and CLI"
```

---

## Task 5: Documentation

Updates `README.md` to list OpenCode everywhere the other three sources are enumerated.

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the intro sources line**

Change the opening description's sources list to include OpenCode. The line currently reads:

```
Sources: **Claude Code**, **Codex CLI**, **Cursor IDE**.
```

becomes:

```
Sources: **Claude Code**, **Codex CLI**, **Cursor IDE**, **OpenCode**.
```

- [ ] **Step 2: Add the data-source table row**

In the "What it does" table, add after the Cursor row:

```
| OpenCode | Parses `~/.local/share/opencode` (`storage/message` JSON + `opencode.db`) for assistant messages with token usage. |
```

- [ ] **Step 3: Refresh the remaining prose enumerations**

Update the other lines that enumerate sources so the docs stay consistent: the opening "merge them with the live Cursor…" sentence, the "reads the same local Claude / Codex / Cursor session files" line, and (if present) add an `OpenCode` row to the example `tokusage show` chart. Use "Claude Code, Codex CLI, Cursor IDE, and OpenCode" phrasing.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document opencode usage source"
```

---

## Final Verification (CI gate parity)

Run the full gate set locally before calling the branch green (matches CI; `cargo fmt --check` is part of CI):

- [ ] `cargo fmt --all` then `cargo fmt --all --check` — Expected: no diff.
- [ ] `cargo clippy --all-targets -- -D warnings` — Expected: no warnings.
- [ ] `cargo test` — Expected: all pass.
- [ ] Re-run the Task 4 Step 4 manual check — Expected: OpenCode row renders, `submit --dry-run` carries `"source":"opencode"`.

---

## Notes for the implementer

- **Pattern to mirror:** `crates/tokusage-core/src/sources/codex.rs` (file walking, `scan` shape, test style) and `crates/tokusage-core/src/sources/cursor.rs:92,297` (read-only SQLite open, `Utc.timestamp_millis_opt(...).single()`).
- **No new dependencies** — `walkdir`, `directories`, `rusqlite`, `chrono`, `serde_json`, `tracing` are already in `tokusage-core`; `tempfile` is a dev-dependency.
- **Error handling discipline:** every per-file / per-row / DB-open failure is `warn!`-logged and skipped; a missing root returns `Ok(vec![])`; the `collect(None)` aggregate logs and skips a whole-source failure. Never propagate an error that would blank the other three sources.
- **YAGNI (per spec):** only the first `OPENCODE_DATA_DIR` entry is honored; no pricing lookup (`cost_cents` comes straight from the stored `cost`).
