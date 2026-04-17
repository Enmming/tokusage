# tokusage backend

Minimum API that receives `POST /api/submit` from the CLI, stores
per-(host, date, client, model, provider) aggregates, and exposes a
`GET /api/summary` sanity endpoint. Point Metabase / Grafana at the
Postgres `daily_usage` table for dashboards.

## Endpoints

| Method | Path | Auth | Purpose |
|---|---|---|---|
| `GET` | `/health` | none | liveness probe |
| `POST` | `/api/submit` | Bearer | accept CLI payload, UPSERT into `daily_usage` |
| `GET` | `/api/summary` | Bearer | simple totals — row count, tokens, cost, unique hosts |

Request body: see the CLI's README.md "Data sent" section. Server is
lenient with extra fields (Pydantic ignores unknowns).

## Storage model

Two tables (see `app/models.py`):

- **`daily_usage`** — canonical aggregate, unique on
  `(host_id, date, client, model, provider)`. UPSERT uses
  `GREATEST(existing, incoming)` per column, so replaying an older payload
  never shrinks recorded totals.
- **`submissions`** — raw ledger of each POST for audit / replay.

Note: the UPSERT expression uses Postgres `ON CONFLICT DO UPDATE`. Tests
run against SQLite in-memory which also supports this syntax; production
targets Postgres 17.

## Local dev

```bash
cp .env.example .env

# Full stack (postgres + api) in docker
docker compose up --build

# Health check
curl http://127.0.0.1:8080/health

# Submit (pretend you're the CLI)
curl -X POST http://127.0.0.1:8080/api/submit \
  -H "Authorization: Bearer devtoken" \
  -H "Content-Type: application/json" \
  -d '{"meta":{...},"contributions":[...]}'

curl http://127.0.0.1:8080/api/summary \
  -H "Authorization: Bearer devtoken"
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

In-memory SQLite. No docker / Postgres needed for CI.

## Auth

Bearer token whitelist via `TOKUSAGE_VALID_TOKENS` (comma-separated).
Replace with a proper tokens table + admin UI in Phase 2.

## Wiring with the CLI

On the employee's machine:
```bash
tokusage login --api-url https://tokusage.yourcorp.com --token <their-token>
```

The CLI sends `Authorization: Bearer <their-token>` on every submit.
