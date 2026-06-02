# tokusage show Command Design

## Goal

Add a local-only `tokusage show` command that renders a simple text chart of
token consumption across the three client sources (Claude Code, Codex CLI,
Cursor IDE), comparing **this calendar month** against **last calendar month**.

It must work offline with no login and no network calls, and it must never
print raw JSON.

## Scope

- New CLI subcommand `show`, no required arguments. Renders once to stdout and
  exits.
- Data source: **local scan only** — reuse the existing per-source scanning
  logic that `submit` already uses. No backend API call.
- Out of scope (YAGNI): interactive TUI, source filtering flags, color flags,
  arbitrary date ranges, per-model breakdown.

## Data Flow

1. **Collect.** Scan all three sources into `Vec<UnifiedMessage>`. The
   orchestration currently lives as a private `collect()` (plus
   `collect_claude/codex/cursor`) in `commands/submit.rs`. Extract these into a
   new shared module `crates/tokusage-cli/src/collect.rs` exposing
   `pub fn collect(source: Option<SourceArg>) -> Result<Vec<UnifiedMessage>>`.
   Both `submit` and `show` call it; `show` passes `None`. Per-source failures
   warn and continue (unchanged behavior).

2. **Aggregate.** `aggregate(messages, now) -> Report`, a pure function with
   `now` injected for testability.
   - Convert each message `timestamp` (UTC) to **local time** and bucket by
     calendar `(year, month)`. "This month" = local `now`'s year-month; "last
     month" = the previous calendar month (handles January → previous December).
   - Accumulate a `TokenBreakdown` per `(client, month-bucket)`.
   - For the current month only, also accumulate a per-day total (index by
     day-of-month) across all clients for the sparkline.

3. **Render.** `render(report) -> String`, pure, returns the full chart text.
   `run()` prints it.

## Render Layout

Chosen style: grouped bars per client (this/last month) + a current-month daily
sparkline + a breakdown footnote. Example:

```
tokusage — token usage (本机)

Claude  Jun ██████████ 2.4M  May ██████ 1.6M
Codex   Jun ████       0.8M  May █████ 1.1M
Cursor  Jun ██         0.3M  May █     0.2M

Daily (Jun): ▁▂▃▅▇▆▃▂▄▅▇█▆▃▂▁▂▃▅▇▆▃▂▄▅
──────────────────────────────────
Total Jun 3.5M  May 2.9M  (+21%)
Jun split: in 0.2M · out 0.3M · cache 3.0M
```

Rules:

- Clients always shown in fixed order: Claude, Codex, Cursor.
- Bar length is normalized against the **max `total` across all 6 bars**
  (3 clients × 2 months) mapped to a fixed max width (~12 chars), so bars are
  comparable across clients and months. Guard against `max == 0`.
- Month labels are the abbreviated month names of the current and previous
  month (e.g. `Jun` / `May`).
- Daily sparkline uses `▁▂▃▄▅▆▇█`, one cell per day of the current month up to
  today. Per-day total normalized against the month's max daily value.
- Footnote `split` reports the current month's `input` / `output` /
  `cache_read + cache_write` sums via `humanize`.
- Percent change = `(total_current - total_last) / total_last`, shown as
  `(+N%)` / `(-N%)`; omit or show `(—)` when last month is zero.

## Components

All in `commands/show.rs` unless noted:

- `run() -> Result<()>` — collect → aggregate → render → print.
- `aggregate(messages: &[UnifiedMessage], now: DateTime<Local>) -> Report`.
- `render(report: &Report) -> String`.
- Helpers: `humanize(i64) -> String` (→ `1.2M`, `12.3K`, raw under 1000),
  `bar(value: i64, max: i64, width: usize) -> String`,
  `sparkline(values: &[i64]) -> String`.
- Structs:
  - `Report { per_client: Vec<ClientMonths>, daily_current: Vec<i64>, current_label: String, last_label: String }`
  - `ClientMonths { client: Client, current: TokenBreakdown, last: TokenBreakdown }`

Shared: `collect.rs` (extracted, described above).

## Empty State

If both months have zero total across all clients, print a friendly message
(e.g. `No local token usage found yet for <last>–<current>. Try 'tokusage
submit' first, or check your tools have run.`) instead of an empty chart. Never
print raw JSON in any path.

## Error Handling

- Per-source scan failures: warn via `tracing` and continue (inherited from the
  extracted `collect`).
- Month arithmetic done on `(year, month)` integers to avoid date-overflow bugs.

## Testing

- `aggregate`: synthetic `UnifiedMessage`s straddling the month boundary and the
  previous month; assert per-client current/last totals and the daily series
  (including a message in the month-before-last is excluded).
- `humanize`: `999` → `999`, `1_000` → `1.0K`, `2_400_000` → `2.4M`, `0` → `0`.
- `bar`: `max == 0` returns empty/zero-width without panic; full value returns
  `width` cells.
- `sparkline`: empty slice, all-zero slice, single value, mixed values.
- `render`: feed a known `Report`; assert key substrings (client labels, month
  labels, humanized totals, the `split:` line). Avoid brittle exact-width
  assertions.

## Wiring & Docs

- `main.rs`: add `Show` to the `Command` enum (doc comment: "Show a local token
  usage chart for this and last month") and route to `commands::show::run()`.
- `commands/mod.rs`: add `pub mod show;`. Add `mod collect;` in `main.rs`.
- `README.md`: add `tokusage show` to the "Ongoing" section.
