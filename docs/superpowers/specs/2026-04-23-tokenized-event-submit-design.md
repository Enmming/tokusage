# Tokenized Event Submit Design

## Goal

Replace the current aggregated submit flow with a token-authenticated raw event ledger.

The new design intentionally keeps `submit` thin:

- authenticate a `user_token`
- accept raw `events[]`
- deduplicate by `event_key`
- record content conflicts for audit
- avoid any aggregation during submit

Statistics are computed later by a separate script directly from stored raw events.

## Naming

The system uses:

- `user`
- `team`
- `user_token`

The design should not introduce `employee` or `company` naming.

## Authentication Model

Each client submits with:

- `Authorization: Bearer <user_token>`

Server behavior:

- store only `token_hash`, never the plaintext token
- resolve the incoming token to one `user_tokens` row
- reject inactive or revoked tokens
- update `last_used_at` on successful authentication

No GitLab integration is part of this design.

## Submit API

### Endpoint

- `POST /api/submit`

### Request body

```json
{
  "client_version": "0.2.0",
  "submitted_at": "2026-04-23T10:30:00Z",
  "events": [
    {
      "source": "claude",
      "event_key": "claude:4d4d5d59-8c2d-4c85-a8b0-3a0d8e8f95cb",
      "event_ts": "2026-04-23T10:28:41Z",
      "session_key": "claude:sha256:abcd1234",
      "seq": 128,
      "model": "claude-opus-4-7",
      "provider": "anthropic",
      "tokens": {
        "input": 6,
        "output": 197,
        "cache_read": 16757,
        "cache_write": 10792,
        "reasoning": 0
      },
      "cost_cents": 0.0,
      "raw_payload": {
        "request_id": "req_abc",
        "message_id": "msg_xyz",
        "uuid": "4d4d5d59-8c2d-4c85-a8b0-3a0d8e8f95cb"
      }
    }
  ]
}
```

### Required top-level fields

- `client_version`
- `submitted_at`
- `events`

### Required event fields

- `source`
- `event_key`
- `event_ts`
- `model`
- `provider`
- `tokens`
- `cost_cents`
- `raw_payload`

### Optional event fields

- `session_key`
- `seq`

`session_key` and `seq` are retained for audit and future evolution, but are not used for order validation in this design.
They are also excluded from duplicate/conflict hashing so the same logical
event can appear in multiple source files without producing a false conflict.

## Event Identity Rules

### Claude

- `event_key = claude:<uuid>`
- `request_id + message_id` is the logical streaming group inside one JSONL file, not the raw event id
- keep only the final snapshot for that logical group within a file
- `session_key = sha256(session file relative path)`
- `seq = line number within the JSONL file`

### Codex

- `turn_id` is the logical turn identity, not the raw event id
- keep every non-empty `token_count` delta as an event
- `event_key = codex:<session_id>:<turn_label>:<timestamp>:<usage_fingerprint>`
- `turn_label = turn_id` when present, otherwise `turn-<turn_index>`
- `session_key = codex:<session_id>:<turn_label>`
- `seq = snapshot index within that logical turn`
- if Codex emits same-timestamp twins with identical usage and only different rate-limit metadata, collapse them to the later line before assigning the event key

### Cursor

- `event_key = cursor:<timestamp>:<owning_user>:<model>:<kind>:<ui_or_headless>`
- `session_key` may be null
- `seq` may be null

## Submit Behavior

For each incoming event:

1. normalize the event content
2. compute `content_hash` from the event content, excluding `session_key` and `seq`
3. look up existing row by `(user_token_id, source, event_key)`

Results:

- no existing row: insert into `raw_usage_events`
- existing row with same `content_hash`: treat as duplicate and ignore
- existing row with different `content_hash`: write audit record to `conflict_events`, then ignore

The endpoint does not:

- aggregate usage
- update `daily_usage`
- perform anomaly detection
- perform sequence/order validation

### Response body

```json
{
  "ok": true,
  "received": 120,
  "inserted": 118,
  "duplicates_ignored": 1,
  "conflicts_ignored": 1
}
```

## Database Design

### `user_tokens`

Purpose: authentication and ownership.

Fields:

- `id`
- `team_id`
- `user_label`
- `token_hash`
- `token_hint`
- `active`
- `created_at`
- `revoked_at`
- `last_used_at`

Constraints:

- unique: `token_hash`

### `raw_usage_events`

Purpose: source of truth for all reporting.

Fields:

- `id`
- `user_token_id`
- `source`
- `event_key`
- `event_ts`
- `session_key` nullable
- `seq` nullable
- `model`
- `provider`
- `input_tokens`
- `output_tokens`
- `cache_read_tokens`
- `cache_write_tokens`
- `reasoning_tokens`
- `cost_cents`
- `content_hash`
- `raw_payload_json`
- `client_version`
- `submitted_at`
- `received_at`

Constraints and indexes:

- unique: `(user_token_id, source, event_key)`
- index: `(user_token_id, event_ts)`
- index: `(source, event_ts)`
- index: `(session_key, seq)`

### `conflict_events`

Purpose: audit records for same-key different-content cases.

Fields:

- `id`
- `user_token_id`
- `source`
- `event_key`
- `existing_event_id`
- `existing_content_hash`
- `incoming_content_hash`
- `incoming_payload_json`
- `client_version`
- `submitted_at`
- `detected_at`
- `reason`

Initial `reason` value:

- `content_mismatch`

## Reporting

`submit` performs no counting or aggregation.

Reporting is handled by a standalone script:

- `backend/scripts/usage_summary.py`

The script reads from `raw_usage_events` and computes statistics on demand.

### Suggested filters

- `--from`
- `--to`
- `--team`
- `--user`
- `--source`
- `--model`
- `--provider`
- `--format table|json|csv`

### Default grouping

- `team_id`
- `user_label`
- `date(event_ts)`
- `source`
- `model`
- `provider`

### Aggregates

- `count(*) as event_count`
- `sum(input_tokens)`
- `sum(output_tokens)`
- `sum(cache_read_tokens)`
- `sum(cache_write_tokens)`
- `sum(reasoning_tokens)`
- `sum(cost_cents)`

`conflict_events` are excluded from reporting.

## Out of Scope

This design intentionally excludes:

- GitLab authentication
- anomaly/outlier validation
- ordering validation
- hot-path aggregation
- `daily_usage` maintenance
- materialized reporting views
