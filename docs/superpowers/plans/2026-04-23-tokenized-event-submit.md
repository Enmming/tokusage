# Tokenized Event Submit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace aggregated usage submit with token-authenticated raw event ingest, conflict auditing, and an offline reporting script.

**Architecture:** The CLI will submit raw `events[]` instead of aggregated contributions. The backend will authenticate with DB-backed `user_tokens`, persist raw events into `raw_usage_events`, record same-key different-content cases in `conflict_events`, and do no aggregation during submit. Reporting moves to a standalone script that reads `raw_usage_events` directly.

**Tech Stack:** Rust CLI/core, FastAPI, SQLAlchemy async ORM, SQLite-backed tests, PostgreSQL in production.

---

## File Structure

### Existing files to modify

- `crates/tokusage-core/src/model.rs`
  Replace aggregate-wire models with raw event submit models.
- `crates/tokusage-core/src/lib.rs`
  Re-export new wire types.
- `crates/tokusage-core/src/sources/claude.rs`
  Emit `event_key`, `session_key`, `seq`, and `raw_payload`.
- `crates/tokusage-core/src/sources/codex.rs`
  Parse `turn_id` when present and emit raw event fields.
- `crates/tokusage-core/src/sources/cursor.rs`
  Emit raw event payload and revised Cursor `event_key`.
- `crates/tokusage-cli/src/commands/submit.rs`
  Stop building aggregated payloads; send raw events.
- `backend/app/schemas.py`
  Replace contribution schema with event schema.
- `backend/app/models.py`
  Replace `daily_usage`/`submissions` with `user_tokens`, `raw_usage_events`, `conflict_events`.
- `backend/app/auth.py`
  Move from env token whitelist to DB-backed token lookup.
- `backend/app/routes.py`
  Rewrite `/api/submit` for raw ingest and remove aggregate logic.
- `backend/tests/test_submit.py`
  Replace upsert/summary tests with ingest/dedup/conflict tests.
- `README.md`
  Update client payload description and onboarding.
- `backend/README.md`
  Update backend schema and reporting guidance.

### New files to create

- `backend/app/security.py`
  Token hashing and token verification helpers.
- `backend/scripts/create_user_token.py`
  Bootstrap script to create one `user_token` row and print the plaintext token once.
- `backend/scripts/usage_summary.py`
  Reporting script that aggregates `raw_usage_events`.
- `backend/tests/test_usage_summary.py`
  Tests for script query behavior or extracted summary helper.

### Files likely to become unused

- `crates/tokusage-core/src/aggregator.rs`
  Remove from submit path; either delete or leave unexported if no longer needed.

### Assumptions locked in for this plan

- `POST /api/submit` remains the only ingest endpoint.
- `/api/summary` is removed rather than reimplemented.
- Reporting happens only through `backend/scripts/usage_summary.py`.
- `session_key` and `seq` are stored but never validated for ordering.

## Task 1: Replace Aggregated Wire Models With Raw Event Models

**Files:**
- Modify: `crates/tokusage-core/src/model.rs`
- Modify: `crates/tokusage-core/src/lib.rs`
- Test: `cargo test`

- [ ] **Step 1: Write the failing compile change by replacing the old submit model definitions**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitEvent {
    pub source: Client,
    pub event_key: String,
    pub event_ts: DateTime<Utc>,
    pub session_key: Option<String>,
    pub seq: Option<u64>,
    pub model: String,
    pub provider: String,
    pub tokens: TokenBreakdown,
    pub cost_cents: f64,
    pub raw_payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitRequest {
    pub client_version: String,
    pub submitted_at: DateTime<Utc>,
    pub events: Vec<SubmitEvent>,
}
```

- [ ] **Step 2: Run compile to verify downstream breakage appears**

Run: `cargo test`  
Expected: FAIL in source scanners / CLI submit because the old `Contribution`, `Meta`, and `SubmitPayload` usages no longer match.

- [ ] **Step 3: Update exports to expose the new request types**

```rust
pub use model::{SubmitEvent, SubmitRequest, TokenBreakdown, UnifiedMessage};
```

- [ ] **Step 4: Run compile again to confirm remaining failures are localized**

Run: `cargo test`
Expected: FAIL only in files still building the old aggregated payload.

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-core/src/model.rs crates/tokusage-core/src/lib.rs
git commit -m "refactor: replace aggregate submit models with raw event models"
```

## Task 2: Extend Source Scanners To Emit Stable Raw Events

**Files:**
- Modify: `crates/tokusage-core/src/sources/claude.rs`
- Modify: `crates/tokusage-core/src/sources/codex.rs`
- Modify: `crates/tokusage-core/src/sources/cursor.rs`
- Test: `cargo test`

- [ ] **Step 1: Write failing tests for Claude/Codex/Cursor event identity fields**

Add assertions like:

```rust
assert_eq!(first.event_key, "claude:req_A:msg_1");
assert_eq!(first.session_key.as_deref(), Some("claude:sha256:..."));
assert_eq!(first.seq, Some(3));
assert_eq!(first.raw_payload["request_id"], "req_A");
```

```rust
assert_eq!(first.event_key, "codex:sess-abc:t1");
assert_eq!(first.session_key.as_deref(), Some("codex:sess-abc"));
assert_eq!(first.seq, Some(0));
```

```rust
assert_eq!(
    first.event_key,
    "cursor:1776348340274:234376495:gpt-5.4-medium:USAGE_EVENT_KIND_INCLUDED_IN_PRO:ui"
);
assert!(first.session_key.is_none());
assert!(first.seq.is_none());
```

- [ ] **Step 2: Run targeted source tests to verify they fail for missing fields**

Run: `cargo test sources:: -- --nocapture`
Expected: FAIL because `UnifiedMessage` does not yet carry `session_key`, `seq`, or `raw_payload`.

- [ ] **Step 3: Extend `UnifiedMessage` and populate all raw event metadata in each scanner**

Minimal shape:

```rust
pub struct UnifiedMessage {
    pub client: Client,
    pub event_key: String,
    pub session_key: Option<String>,
    pub seq: Option<u64>,
    pub model: String,
    pub provider: String,
    pub timestamp: DateTime<Utc>,
    pub tokens: TokenBreakdown,
    pub cost_cents: f64,
    pub raw_payload: serde_json::Value,
}
```

Claude specifics:

```rust
event_key: format!("claude:{}:{}", request_id, msg_id),
session_key: Some(format!("claude:sha256:{}", short_hash(relative_path))),
seq: Some(line_no as u64),
raw_payload: serde_json::json!({
    "request_id": request_id,
    "message_id": msg_id
}),
```

Codex specifics:

```rust
event_key: if let Some(turn_id) = turn_id.as_deref() {
    format!("codex:{}:{}", sid, turn_id)
} else {
    format!("codex:{}:{}:{}", sid, ts_str, model)
},
session_key: Some(format!("codex:{}", sid)),
seq: Some(turn_index as u64),
raw_payload: serde_json::json!({
    "session_id": sid,
    "turn_id": turn_id,
    "turn_index": turn_index
}),
```

Cursor specifics:

```rust
event_key: format!(
    "cursor:{}:{}:{}:{}:{}",
    ev.timestamp,
    ev.owning_user,
    ev.model,
    ev.kind,
    if ev.is_headless { "hl" } else { "ui" }
),
session_key: None,
seq: None,
raw_payload: serde_json::to_value(&ev).unwrap_or(serde_json::json!({})),
```

- [ ] **Step 4: Run all Rust tests to verify scanners still parse correctly**

Run: `cargo test`
Expected: PASS for source parser tests, with any remaining failures isolated to CLI submit or removed aggregator callers.

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-core/src/model.rs crates/tokusage-core/src/sources/claude.rs crates/tokusage-core/src/sources/codex.rs crates/tokusage-core/src/sources/cursor.rs
git commit -m "feat: emit raw usage event metadata from source scanners"
```

## Task 3: Rework CLI Submit To Send Raw Events

**Files:**
- Modify: `crates/tokusage-cli/src/commands/submit.rs`
- Modify: `crates/tokusage-core/src/aggregator.rs` or delete it
- Test: `cargo test`
- Test: `cargo run -- submit --dry-run`

- [ ] **Step 1: Write the failing dry-run expectation for raw event payload output**

Target shape:

```json
{
  "client_version": "0.2.0",
  "submitted_at": "2026-04-23T10:30:00Z",
  "events": [
    {
      "source": "claude",
      "event_key": "claude:req_A:msg_1",
      "event_ts": "2026-04-23T10:28:41Z"
    }
  ]
}
```

- [ ] **Step 2: Run the dry-run command to confirm it still prints the old aggregate payload**

Run: `cargo run -- submit --source claude --dry-run`
Expected: FAIL against the new expected shape because output still contains `meta` and `contributions`.

- [ ] **Step 3: Replace aggregate build logic with raw request construction**

Implementation target:

```rust
let payload = SubmitRequest {
    client_version: env!("CARGO_PKG_VERSION").to_string(),
    submitted_at: chrono::Utc::now(),
    events: messages.into_iter().map(to_submit_event).collect(),
};
```

If `aggregator.rs` becomes unused:

- delete it and remove `pub mod aggregator;`
- or leave it only if another path still needs it

- [ ] **Step 4: Run CLI and test suite**

Run: `cargo test`
Expected: PASS

Run: `cargo run -- submit --source claude --dry-run`
Expected: Raw event JSON with `events[]`

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-cli/src/commands/submit.rs crates/tokusage-core/src/lib.rs crates/tokusage-core/src/aggregator.rs
git commit -m "refactor: submit raw usage events from cli"
```

## Task 4: Add DB-Backed User Token Authentication

**Files:**
- Create: `backend/app/security.py`
- Modify: `backend/app/models.py`
- Modify: `backend/app/auth.py`
- Modify: `backend/app/routes.py`
- Test: `backend/tests/test_submit.py`

- [ ] **Step 1: Write failing backend tests for DB-backed token auth**

Add tests that require:

```python
assert r.status_code == 401
```

for unknown tokens, and:

```python
assert r.status_code == 200
```

for a token inserted into `user_tokens`.

- [ ] **Step 2: Run backend tests to confirm env-whitelist auth no longer matches desired behavior**

Run: `cd backend && .venv/bin/pytest tests/test_submit.py -v`
Expected: FAIL because auth still reads `TOKUSAGE_VALID_TOKENS`.

- [ ] **Step 3: Implement `user_tokens` model and hashed-token verification**

Suggested helper:

```python
import hashlib

def token_hash(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()
```

Suggested auth flow:

```python
async def require_user_token(
    authorization: str | None = Header(default=None),
    session: AsyncSession = Depends(get_session),
) -> UserToken:
    token = extract_bearer(authorization)
    hashed = token_hash(token)
    row = await session.scalar(
        select(UserToken).where(UserToken.token_hash == hashed, UserToken.active.is_(True))
    )
    if row is None:
        raise HTTPException(status_code=401, detail="invalid token")
    row.last_used_at = func.now()
    return row
```

- [ ] **Step 4: Run backend token-auth tests**

Run: `cd backend && .venv/bin/pytest tests/test_submit.py -v`
Expected: PASS for auth cases; ingest route may still fail until schema rewrite is complete.

- [ ] **Step 5: Commit**

```bash
git add backend/app/security.py backend/app/models.py backend/app/auth.py backend/app/routes.py backend/tests/test_submit.py
git commit -m "feat: add db-backed user token authentication"
```

## Task 5: Rewrite Backend Submit Ingest For Raw Events and Conflict Audit

**Files:**
- Modify: `backend/app/schemas.py`
- Modify: `backend/app/models.py`
- Modify: `backend/app/routes.py`
- Modify: `backend/tests/test_submit.py`
- Test: `cd backend && .venv/bin/pytest tests/test_submit.py -v`

- [ ] **Step 1: Write failing tests for raw ingest, duplicate ignore, and conflict audit**

Tests to add:

```python
async def test_insert_new_events(client): ...
async def test_duplicate_events_are_ignored(client): ...
async def test_conflicting_events_are_audited_and_ignored(client): ...
```

Expected response shape:

```python
assert body == {
    "ok": True,
    "received": 2,
    "inserted": 1,
    "duplicates_ignored": 1,
    "conflicts_ignored": 0,
}
```

- [ ] **Step 2: Run tests to verify old contribution schema no longer fits**

Run: `cd backend && .venv/bin/pytest tests/test_submit.py -v`
Expected: FAIL because schemas and route still expect `meta` / `contributions`.

- [ ] **Step 3: Implement new schemas and route behavior**

Schema target:

```python
class SubmitEvent(BaseModel):
    source: Literal["claude", "codex", "cursor"]
    event_key: str
    event_ts: dt.datetime
    session_key: str | None = None
    seq: int | None = None
    model: str
    provider: str
    tokens: TokenBreakdown
    cost_cents: float = 0.0
    raw_payload: dict

class SubmitRequest(BaseModel):
    client_version: str
    submitted_at: dt.datetime
    events: list[SubmitEvent]
```

Route target:

```python
existing = await session.scalar(
    select(RawUsageEvent).where(
        RawUsageEvent.user_token_id == user_token.id,
        RawUsageEvent.source == event.source,
        RawUsageEvent.event_key == event.event_key,
    )
)
if existing is None:
    session.add(...)
elif existing.content_hash == incoming_hash:
    duplicates_ignored += 1
else:
    session.add(ConflictEvent(...))
    conflicts_ignored += 1
```

Also remove `/api/summary`.

- [ ] **Step 4: Run backend ingest tests**

Run: `cd backend && .venv/bin/pytest tests/test_submit.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/app/schemas.py backend/app/models.py backend/app/routes.py backend/tests/test_submit.py
git commit -m "refactor: ingest raw usage events with conflict auditing"
```

## Task 6: Add Token Bootstrap Script

**Files:**
- Create: `backend/scripts/create_user_token.py`
- Modify: `backend/README.md`
- Test: `python backend/scripts/create_user_token.py --help`

- [ ] **Step 1: Write the script interface and expected one-time output**

CLI target:

```bash
python backend/scripts/create_user_token.py --team team-a --user alice@example.com
```

Expected output:

```text
Plaintext token (shown once): tk_...
Store this now; only the hash is persisted.
```

- [ ] **Step 2: Run `--help` to verify the script does not yet exist**

Run: `python backend/scripts/create_user_token.py --help`
Expected: FAIL with file not found.

- [ ] **Step 3: Implement token generation and DB insert**

Suggested core:

```python
plaintext = secrets.token_urlsafe(32)
hashed = token_hash(plaintext)
hint = f"{plaintext[:4]}…{plaintext[-4:]}"
```

Insert one `UserToken` row with `team_id`, `user_label`, `token_hash`, `token_hint`, `active=True`.

- [ ] **Step 4: Run script help and a local dry invocation**

Run: `python backend/scripts/create_user_token.py --help`
Expected: PASS with usage text

- [ ] **Step 5: Commit**

```bash
git add backend/scripts/create_user_token.py backend/README.md
git commit -m "feat: add user token bootstrap script"
```

## Task 7: Add On-Demand Reporting Script

**Files:**
- Create: `backend/scripts/usage_summary.py`
- Create: `backend/tests/test_usage_summary.py`
- Modify: `backend/README.md`
- Test: `cd backend && .venv/bin/pytest tests/test_usage_summary.py -v`

- [ ] **Step 1: Write failing tests for grouped summary output**

Target cases:

```python
def test_groups_by_team_user_date_source_model_provider(): ...
def test_filters_by_date_range_and_source(): ...
```

- [ ] **Step 2: Run tests to verify the script/helper does not exist**

Run: `cd backend && .venv/bin/pytest tests/test_usage_summary.py -v`
Expected: FAIL because module or helper is missing.

- [ ] **Step 3: Implement summary query and output formats**

Suggested SQL shape:

```sql
SELECT
  ut.team_id,
  ut.user_label,
  DATE(rue.event_ts) AS usage_date,
  rue.source,
  rue.model,
  rue.provider,
  COUNT(*) AS event_count,
  SUM(rue.input_tokens) AS input_tokens,
  SUM(rue.output_tokens) AS output_tokens,
  SUM(rue.cache_read_tokens) AS cache_read_tokens,
  SUM(rue.cache_write_tokens) AS cache_write_tokens,
  SUM(rue.reasoning_tokens) AS reasoning_tokens,
  SUM(rue.cost_cents) AS cost_cents
FROM raw_usage_events rue
JOIN user_tokens ut ON ut.id = rue.user_token_id
WHERE ...
GROUP BY ut.team_id, ut.user_label, DATE(rue.event_ts), rue.source, rue.model, rue.provider
ORDER BY usage_date, ut.team_id, ut.user_label, rue.source, rue.model;
```

Expose `--format table|json|csv`.

- [ ] **Step 4: Run reporting tests**

Run: `cd backend && .venv/bin/pytest tests/test_usage_summary.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add backend/scripts/usage_summary.py backend/tests/test_usage_summary.py backend/README.md
git commit -m "feat: add raw event usage summary script"
```

## Task 8: Update Product and Developer Docs

**Files:**
- Modify: `README.md`
- Modify: `backend/README.md`
- Test: manual doc review

- [ ] **Step 1: Update root README to describe raw event submit instead of aggregate contributions**

Replace:

- `meta.host_id`
- `contributions[]`
- `daily_usage` aggregate semantics

With:

- token-authenticated `events[]`
- raw ingest / conflict audit behavior
- reporting script usage

- [ ] **Step 2: Update backend README for the new storage model**

Document:

- `user_tokens`
- `raw_usage_events`
- `conflict_events`
- `create_user_token.py`
- `usage_summary.py`

- [ ] **Step 3: Review docs for stale aggregate terminology**

Run: `rg -n "host_id|contributions|daily_usage|summary" README.md backend/README.md backend/app`
Expected: only intentional mentions remain.

- [ ] **Step 4: Commit**

```bash
git add README.md backend/README.md
git commit -m "docs: describe raw event submit and reporting flow"
```

## Task 9: Final Verification

**Files:**
- Test: Rust workspace
- Test: Backend tests
- Test: CLI dry-run
- Test: Reporting script help

- [ ] **Step 1: Run Rust tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 2: Run backend tests**

Run: `cd backend && .venv/bin/pytest`
Expected: PASS

- [ ] **Step 3: Verify CLI raw submit output**

Run: `cargo run -- submit --source claude --dry-run`
Expected: JSON with top-level `client_version`, `submitted_at`, and `events`

- [ ] **Step 4: Verify reporting script help**

Run: `python backend/scripts/usage_summary.py --help`
Expected: PASS with filter and format options

- [ ] **Step 5: Commit final integration state**

```bash
git add -A
git commit -m "feat: switch to tokenized raw event ingest"
```
