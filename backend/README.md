# tokusage backend

Minimum API that receives `POST /api/submit` from the CLI and stores raw
usage events. Per-user reporting is available through `GET /api/summary`;
team-wide/operator reporting is available through a summary script that
recomputes totals from `raw_usage_events`.

## Endpoints

| Method | Path | Auth | Purpose |
|---|---|---|---|
| `GET` | `/health` | none | liveness probe |
| `POST` | `/api/submit` | Bearer | accept CLI payload, store raw events, ignore duplicates, audit conflicts |
| `GET` | `/api/summary` | Bearer | return daily usage summary rows for the authenticated user |

Request body shape:

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

## Storage model

Three tables (see `app/models.py`):

- **`user_tokens`** — bearer-token registry. The service stores
  `token_hash`, `token_hint`, `team_id`, `user_label`, and lifecycle fields.
- **`raw_usage_events`** — the source of truth. Unique on
  `(user_token_id, source, event_key)`.
- **`conflict_events`** — audit table for "same key, different content".

`session_key` and `seq` are retained on each raw event for audit, but are
excluded from duplicate/conflict hashing so the same logical event can be
mirrored across multiple source files without producing false conflicts.

Submit requests are bounded before storage:

- maximum request body: 8 MiB by default (`TOKUSAGE_MAX_REQUEST_BYTES`)
- maximum `events`: 1000 per submit
- token counts, sequence numbers, and costs must be non-negative
- each event `raw_payload` is capped at 64 KiB

Current source semantics:

- `Claude` emits one final event per logical assistant response. The raw unique id is the transcript row `uuid`, while `request_id + message_id` is only the in-file streaming group used to pick the final snapshot.
- `Codex` emits one event per non-empty `token_count` delta. The stored `event_key` is a synthesized composite of `session`, logical turn label, `timestamp`, and a usage fingerprint; same-timestamp twin emissions with identical usage are collapsed before insert.
- `Cursor` emits one event per dashboard usage row. The stored `event_key` is `timestamp + owning_user + model + kind + ui/headless`, which is the best identity available in Cursor's current payload shape.

`POST /api/submit` does not aggregate and does not maintain a precomputed
summary table. `GET /api/summary` computes rows from `raw_usage_events` on
demand for the authenticated user.

## Local dev

```bash
cp .env.example .env

# Full stack (postgres + api) in docker
docker compose up --build

# Health check
curl http://127.0.0.1:8080/health

# Create a user token
.venv/bin/python scripts/create_user_token.py \
  --team team-a \
  --user alice

# Submit (pretend you're the CLI)
curl -X POST http://127.0.0.1:8080/api/submit \
  -H "Authorization: Bearer <plain-token-from-create_user_token>" \
  -H "Content-Type: application/json" \
  -d '{"client_version":"0.2.0","submitted_at":"2026-04-23T10:30:00Z","events":[...]}'

# Read the authenticated user's summary
curl "http://127.0.0.1:8080/api/summary?from=2026-04-01&to=2026-04-30" \
  -H "Authorization: Bearer <plain-token-from-create_user_token>"

# Recompute team-wide summaries on demand
.venv/bin/python scripts/usage_summary.py --from 2026-04-01 --to 2026-04-30
```

## Bare metal (no docker)

```bash
uv venv && source .venv/bin/activate
uv pip install -e . --group dev

# Start Postgres separately (docker compose up postgres)
export $(grep -v '^#' .env | xargs)
uvicorn app.main:app --reload --port 8080
```

## Tests

```bash
uv pip install -e . --group dev
pytest
```

SQLite. No docker / Postgres needed for CI.

## Auth

Bearer tokens are backed by the `user_tokens` table. Use
`scripts/create_user_token.py` to create a token and hand the
plain token to the user once.

## Wiring with the CLI

On the user's machine:
```bash
tokusage login --api-url https://tokusage.yourteam.internal --token <user-token>
```

The CLI sends `Authorization: Bearer <user-token>` on every submit.
