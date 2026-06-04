# OpenCode Usage Source Design

## Goal

Add [opencode](https://opencode.ai/) as a fourth local usage source alongside
Claude Code, Codex CLI, and Cursor IDE. After this change, `tokusage show`,
`tokusage submit`, and `tokusage submit --source opencode` all include opencode
token usage parsed from the user's local opencode data — no network, no login.

The source mirrors the existing `sources/codex.rs` pattern: a file/DB reader in
`tokusage-core` that produces `UnifiedMessage`s, wired through `collect.rs`,
bucketed by the `Client` enum, and rendered in `show.rs`.

## Reference

opencode's on-disk format was derived from
[ccusage's Rust opencode adapter](https://github.com/ryoppippi/ccusage)
(`rust/crates/ccusage/src/adapter/opencode/`), which reads the same files.

## Storage Format

opencode stores data under `OPENCODE_DATA_DIR` (default
`~/.local/share/opencode`). Usage lives in two tiers; opencode migrated from
JSON files to SQLite, keeping JSON for backward compatibility, so both can be
present:

1. **Legacy JSON** — `storage/message/{sessionID}/msg_{messageID}.json`, one
   assistant message per file.
2. **SQLite** — `opencode.db` (or a channel DB `opencode-*.db`) with a
   `message(id, session_id, data)` table, where `data` is the same message JSON
   string.

Both tiers can hold the same message, so entries are **deduplicated by message
id**.

### Assistant message JSON → `TokenBreakdown`

```json
{
  "id": "msg_...",
  "sessionID": "ses_...",
  "providerID": "anthropic",
  "modelID": "claude-sonnet-4-20250514",
  "time": { "created": 1767312000000 },
  "tokens": { "input": 100, "output": 50, "reasoning": 0,
              "cache": { "read": 10, "write": 20 } },
  "cost": 0
}
```

| opencode field        | tokusage field            |
|-----------------------|---------------------------|
| `tokens.input`        | `tokens.input`            |
| `tokens.output`       | `tokens.output`           |
| `tokens.cache.read`   | `tokens.cache_read`       |
| `tokens.cache.write`  | `tokens.cache_write`      |
| `tokens.reasoning`    | `tokens.reasoning`        |
| `modelID` (required)  | `model`                   |
| `providerID` (req'd)  | `provider`                |
| `time.created` (ms)   | `timestamp` (`DateTime<Utc>`) |
| `cost` (USD)          | `cost_cents = cost * 100.0` |
| `id`                  | `event_key = "opencode:{id}"` |
| `sessionID`           | `session_key = "opencode:{sessionID}"` |

A message is **skipped** when all five token fields are zero, when `modelID` or
`providerID` is missing/empty, or when `time.created` is absent/unparseable
(parsed via `Utc.timestamp_millis_opt(ms).single()`, mirroring `cursor.rs`'s
early-return). `seq` is `None`.

## Components

### 1. `tokusage-core/src/model.rs`
Add `OpenCode` to the `Client` enum. The existing
`#[serde(rename_all = "lowercase")]` serializes it as `"opencode"`; `as_str()`
returns `"opencode"`.

### 2. `tokusage-core/src/sources/opencode.rs` (new)
Mirrors `codex.rs`. Public API:

- `default_root() -> Option<PathBuf>`: returns the first comma-separated entry of
  `OPENCODE_DATA_DIR` if set, otherwise `~/.local/share/opencode`.
- `scan(root: &Path) -> ScanResult`: returns `Ok(vec![])` if `root` is absent.
  Reads both tiers and dedupes by `event_key`:
  1. **SQLite**: open `opencode.db` / first `opencode-*.db` read-only via
     `rusqlite` (already a workspace dependency), then
     `SELECT id, session_id, data FROM message`. A missing file, missing
     `message` table, locked DB, or unparseable `data` row → `warn!` + skip;
     never fails the scan.
  2. **JSON**: walk `storage/message/**/*.json` with `walkdir`, one message per
     file; unreadable/unparseable files → `warn!` + skip.

- Private `message_value_to_unified(value, id, session_id) -> Option<UnifiedMessage>`
  shared by both tiers, applying the mapping table above. The message id used for
  `event_key`/dedup is the in-file `id` field, which equals the DB `id` exactly,
  so the same message in both tiers collides. The JSON tier falls back to the
  file stem only when the in-file `id` is absent — the stem keeps the `msg_`
  prefix so it still lines up with the DB `id`. `session_id` comes from the DB
  row, else JSON `sessionID`. `raw_payload` carries `{ session_id, message_id,
  provider, tier: "db"|"json" }`.

Dedup: a `HashSet<String>` of seen `event_key`s. The **DB tier is read first**
(it is the newer, migrated copy, so on a JSON/DB collision the DB row wins), then
JSON, skipping already-seen ids.

### 3. `tokusage-core/src/sources/mod.rs`
Add `pub mod opencode;`.

### 4. `tokusage-cli/src/collect.rs`
Add `collect_opencode()` and wire it into both the
`Some(SourceArg::OpenCode)` arm and the `None` aggregate. In the aggregate, a
source failure is logged via `tracing::warn!` and skipped, like the others.

### 5. `tokusage-cli/src/main.rs`
Add `OpenCode` to `SourceArg`. Update **both** the hand-written `--source` doc
comment (`/// Only run a single source (claude|codex|cursor)` → add `|opencode`)
and the top-level `about` string to mention opencode.

### 6. `tokusage-cli/src/commands/show.rs`
- Add `Client::OpenCode` to the `order` array.
- `client_name(Client::OpenCode)` → `"OpenCode"`.
- Widen the client-name column in `render` from `{:<7}` to `{:<8}` (so the
  8-char "OpenCode" label does not push the bars out of alignment).

## Error Handling

Consistent with existing sources: per-file and per-row parse failures are logged
and skipped; a missing root returns an empty vec; a whole-source failure in the
`collect(None)` aggregate is logged and skipped so the other three sources still
render.

## Testing

New `sources/opencode.rs` unit tests (using `tempfile`, mirroring `codex.rs`):

1. `parses_message_json_file` — a `storage/message/.../msg.json` yields one
   `UnifiedMessage` with the correct breakdown, model, provider, timestamp,
   and `event_key`.
2. `parses_sqlite_message` — a temp `opencode.db` with one `message` row parses
   correctly.
3. `dedupes_json_and_db_by_id` — the same message id present in both tiers
   yields exactly one message.
4. `skips_zero_tokens_and_missing_model` — all-zero tokens, or absent
   `modelID`/`providerID`, are skipped.
5. `missing_root_returns_empty`.
6. `default_root_respects_env_first_entry` — `OPENCODE_DATA_DIR="a,b"` resolves
   to `a`.
7. `db_without_message_table_is_skipped` — an `opencode.db` that lacks the
   `message` table yields an empty result and no error (the backward-compat
   case for older databases).

Update `show.rs` tests (`aggregate_buckets_by_client_and_month`,
`render_contains_key_lines`) to include `Client::OpenCode` and assert
`"OpenCode"` renders.

## Documentation

`README.md`:
- Add **OpenCode** to the intro "Sources:" line.
- Add a row to the data-source table:
  `| OpenCode | Parses ~/.local/share/opencode (storage/message JSON + opencode.db) for assistant messages with token usage. |`
- Refresh the other source enumerations so the docs stay consistent: the opening
  "merge them with the live Cursor…" sentence, the "reads the same local Claude /
  Codex / Cursor session files" line, and the example chart (add an OpenCode row).

## Out of Scope (YAGNI)

- **Multi-directory `OPENCODE_DATA_DIR` lists** — v1 reads only the first entry.
- **Cost/pricing lookup** — `cost_cents` derives solely from opencode's stored
  `cost` field, which may be `0` or a value opencode computed locally. tokusage
  does no LiteLLM/pricing lookup of its own; any repricing remains the backend's
  job.
