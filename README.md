# tokusage

Scan your local AI coding tool session files, merge them with the live Cursor
dashboard API, and POST aggregated usage to your company's internal endpoint
on a schedule. Sources: **Claude Code**, **Codex CLI**, **Cursor IDE**.

No cookies to copy, no dashboards to open — once a day you run `tokusage init`
and the rest is automatic.

## What it does

For each AI tool:

| Tool | How tokusage gets the data |
|---|---|
| Claude Code | Parses `~/.claude/projects/**/*.jsonl` for assistant entries with `usage`. |
| Codex CLI   | Parses `$CODEX_HOME/sessions/**/*.jsonl` for per-turn `last_token_usage`. |
| Cursor IDE  | Reads the JWT Cursor IDE stores in its SQLite state DB, then calls `api2.cursor.sh/aiserver.v1.DashboardService/GetFilteredUsageEvents`. |

All three are normalized into a single payload and POSTed to your
configured endpoint every 30 minutes (via `launchd`).

## Install

```bash
curl -sSL https://github.com/gd/tokusage/releases/latest/download/install.sh | bash
```

Downloads the right platform binary, verifies sha256, installs to
`~/.local/bin/tokusage`, strips macOS Gatekeeper quarantine.

Pin a version with `TOKUSAGE_VERSION=v0.1.0`; override the install directory
with `TOKUSAGE_BIN_DIR=...`.

## First-time setup

```bash
tokusage login   # enter your company API URL and token (saved to ~/.config/tokusage/config.toml)
tokusage init    # install launchd scheduler; optionally inject Claude Code Stop hook
tokusage submit  # send the first payload immediately
```

## Ongoing

```bash
tokusage status       # show config, install state, queued retries, last run time
tokusage submit       # run once on demand
tokusage self-update  # fetch latest release and re-install
```

## Uninstall

```bash
tokusage self-uninstall
```

Removes the launchd agent, Claude Code hook (if installed), config, data
directory, and queue. The binary itself is left for you to remove.

## Paths

| What | Where |
|---|---|
| Binary | `~/.local/bin/tokusage` |
| Config | `~/.config/tokusage/config.toml` |
| Data (manifest, queue, logs) | `~/.local/share/tokusage/` |
| launchd plist | `~/Library/LaunchAgents/com.gd.tokusage.plist` |
| Run log (launchd stdout/stderr) | `~/.local/share/tokusage/logs/submit.log` |

## Data sent

Every 30 minutes tokusage POSTs a JSON payload to
`<api_url>/api/submit` with `Authorization: Bearer <token>`:

```json
{
  "meta": {
    "generated_at": "2026-04-17T10:30:00Z",
    "client_version": "0.1.0",
    "host_id": "38b3310301759227",
    "date_range": { "start": "2026-04-17", "end": "2026-04-17" }
  },
  "contributions": [
    {
      "date": "2026-04-17",
      "client": "claude",
      "model": "claude-opus-4-7",
      "provider": "anthropic",
      "tokens": { "input": 6, "output": 197, "cache_read": 16757, "cache_write": 10792, "reasoning": 0 },
      "cost_cents": 0.0,
      "message_count": 1,
      "dedup_keys": ["claude:req_xxx:msg_yyy"]
    }
  ]
}
```

`host_id` is `sha256(username:hostname)` truncated to 16 hex chars — no
raw username is sent.

Server-side UPSERT on `dedup_keys` handles cross-submission dedup so the
client can stay stateless and idempotent.

## Dev

```bash
cargo build                                      # workspace compile
cargo test                                       # all tests
cargo run -- submit --source claude --dry-run   # print Claude-only payload
python3 scripts/mock-server.py 8080 &           # start local mock endpoint
```

## License

MIT.
