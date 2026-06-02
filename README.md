# tokusage

Scan your local AI coding tool session files, merge them with the live Cursor
dashboard API, and POST raw usage events to your team's internal endpoint
on a schedule. Sources: **Claude Code**, **Codex CLI**, **Cursor IDE**.

No cookies to copy, no dashboards to open — run `tokusage login` once, then
`tokusage init`, and the rest is automatic.

Want a quick local glance without the backend? `tokusage show` draws a
month-over-month usage chart straight from your local session files — no
network, no login.

## What it does

For each AI tool:

| Tool | How tokusage gets the data |
|---|---|
| Claude Code | Parses `~/.claude/projects/**/*.jsonl` for assistant entries with `usage`. |
| Codex CLI   | Parses `$CODEX_HOME/sessions/**/*.jsonl` for non-empty `last_token_usage` snapshots. |
| Cursor IDE  | Reads the JWT Cursor IDE stores in its SQLite state DB, then calls `api2.cursor.sh/aiserver.v1.DashboardService/GetFilteredUsageEvents`. |

All three are normalized into a single payload and POSTed to your
configured endpoint every 30 minutes.

## Install

```bash
curl -sSL https://github.com/Enmming/tokusage/releases/latest/download/install.sh | bash
```

Downloads the right platform binary, verifies sha256, installs to
`~/.local/bin/tokusage`, strips macOS Gatekeeper quarantine.

Pin a version with `TOKUSAGE_VERSION=v0.2.0`; override the install directory
with `TOKUSAGE_BIN_DIR=...`. The installer requires the release `.sha256`
sidecar unless `TOKUSAGE_SKIP_CHECKSUM=1` is explicitly set.

## First-time setup

```bash
tokusage login   # enter your team's API URL and user token (saved to ~/.config/tokusage/config.toml)
tokusage init    # install the scheduler; optionally inject Claude Code Stop hook
tokusage submit  # send the first payload immediately
```

## Ongoing

```bash
tokusage status       # show config, install state, queued retries, last run time
tokusage show         # local chart of this vs last month token usage (no network)
tokusage submit       # run once on demand
tokusage self-update  # fetch latest release and re-install
```

## View usage locally

`tokusage show` reads the same local Claude / Codex / Cursor session files as
`submit` and renders a plain-text chart comparing **this calendar month** with
**last month**. It runs fully offline — no login, no network — and never prints
raw JSON.

```text
$ tokusage show
tokusage — token usage (local)

Claude  Jun ████████████  2.4M  May ████████      1.6M
Codex   Jun ████          0.8M  May ██████        1.1M
Cursor  Jun ██            0.3M  May █             0.2M

Daily (Jun): ▁▂▃▅▇▆▃▂▄▅▇█▆▃▂▁
──────────────────────────────────
Total Jun 3.5M  May 2.9M  (+21%)
Jun split: in 0.2M · out 0.3M · cache 3.0M
```

- One row per source; the two bars are this month vs last month, scaled against
  a shared maximum so heights are comparable across sources and months.
- `Daily` is a sparkline of the current month's per-day totals, up to today.
- `Total` sums all sources, with the month-over-month change in parentheses.
- `split` breaks the current month into input / output / cache tokens.

If no local usage is found yet, `show` prints a short hint instead of an empty
chart.

## Uninstall

```bash
tokusage self-uninstall
```

Removes the scheduler, Claude Code hook (if installed), config, data directory,
and queue. The binary itself is left for you to remove.

## Paths

| What | Where |
|---|---|
| Binary | `~/.local/bin/tokusage` |
| Config | `~/.config/tokusage/config.toml` |
| Data (manifest, queue, logs) | `~/.local/share/tokusage/` |
| macOS launchd plist | `~/Library/LaunchAgents/com.Enmming.tokusage.plist` |
| Linux systemd timer | `~/.config/systemd/user/tokusage.timer` |
| Windows Task Scheduler task | `Tokusage` |
| Run log (scheduler stdout/stderr) | `~/.local/share/tokusage/logs/submit.log` |

## Data sent

Every 30 minutes tokusage POSTs a JSON payload to
`<api_url>/api/submit` with `Authorization: Bearer <user_token>`:

```json
{
  "client_version": "0.3.0",
  "submitted_at": "2026-04-17T10:30:00Z",
  "events": [
    {
      "source": "claude",
      "event_key": "claude:4d4d5d59-8c2d-4c85-a8b0-3a0d8e8f95cb",
      "event_ts": "2026-04-17T10:29:58Z",
      "session_key": "claude:sha256:fc378a709b7d6f3aad1c8d1cc459e1b10ba6685b2ea5a7fe7a143d95fa6f4237",
      "seq": 128,
      "model": "claude-opus-4-7",
      "provider": "anthropic",
      "tokens": { "input": 6, "output": 197, "cache_read": 16757, "cache_write": 10792, "reasoning": 0 },
      "cost_cents": 0.0,
      "raw_payload": { "request_id": "req_xxx", "message_id": "msg_yyy", "uuid": "4d4d5d59-8c2d-4c85-a8b0-3a0d8e8f95cb" }
    }
  ]
}
```

The backend stores raw events, ignores exact duplicates for the same
`event_key`, and audits same-key/different-content conflicts separately.
`session_key` and `seq` are stored for audit, but do not affect
duplicate/conflict classification.

Per-source identity rules:

- `Claude`: `event_key` is the assistant row `uuid`. tokusage still groups by `requestId + message.id` inside one JSONL file so streamed snapshots collapse to the final row before submit.
- `Codex`: `event_key` is `session + logical turn + timestamp + usage fingerprint`. Multiple non-empty `token_count` deltas from the same turn are preserved; same-timestamp twins with identical usage are collapsed before submit.
- `Cursor`: `event_key` is `timestamp + owningUser + model + kind + ui/headless`. That is the best identity Cursor currently exposes in its usage payload; `session_key` and `seq` stay null.

Cursor connectivity notes:

- tokusage bypasses system proxies for Cursor by default because `reqwest + rustls` frequently fails on proxy TLS handshakes while direct access to `api2.cursor.sh` still works.
- If your network really requires a proxy for Cursor, set `TOKUSAGE_CURSOR_USE_PROXY=1`.
- Cursor state DB paths are detected per platform: macOS
  `~/Library/Application Support/Cursor/...`, Linux `~/.config/Cursor/...`,
  and Windows `%APPDATA%\Cursor\...`.

## Reporting

The backend exposes `GET /api/summary` for the authenticated user's own usage:

```bash
curl https://tokusage.yourteam.internal/api/summary?from=2026-04-01\&to=2026-04-30 \
  -H "Authorization: Bearer <user-token>"
```

Backend operators can also run `backend/scripts/usage_summary.py` for team-wide
CSV, JSON, or table exports.

## Backend deployment

The backend is a FastAPI service with a Postgres database. GitHub Actions
publishes the service image to GHCR as:

```text
ghcr.io/enmming/tokusage-backend:latest
ghcr.io/enmming/tokusage-backend:<release-tag>
```

Use `backend/docker-compose.prod.yml` on the server:

```bash
cd backend
cp .env.prod.example .env
$EDITOR .env
docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml up -d
curl http://127.0.0.1:8080/health
```

See `backend/README.md` for token creation and reverse proxy notes.

## Dev

```bash
cargo build                                      # workspace compile
cargo test                                       # all tests
cargo run -- submit --source claude --dry-run   # print Claude-only payload
python3 scripts/mock-server.py 8080 &           # start local mock endpoint
```

## License

MIT.
