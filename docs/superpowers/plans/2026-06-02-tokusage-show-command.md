# tokusage show Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an offline `tokusage show` command that renders a plain-text bar chart of token usage across Claude/Codex/Cursor, comparing this calendar month to last.

**Architecture:** Reuse the existing local source-scan logic (extracted into a shared `collect` module). A pure `aggregate()` buckets messages into per-client this-month/last-month `TokenBreakdown`s plus a current-month daily series; a pure `render()` turns that into the chart string; `run()` wires scan → aggregate → render → print. No network, no JSON output.

**Tech Stack:** Rust, clap (subcommands), chrono (`Local`, `Datelike`), existing `tokusage_core` sources.

---

## File Structure

- `crates/tokusage-cli/src/collect.rs` — **new**, shared source-scan orchestration (`pub fn collect`).
- `crates/tokusage-cli/src/commands/submit.rs` — **modify**, delegate to `crate::collect::collect`.
- `crates/tokusage-cli/src/commands/show.rs` — **new**, the command: helpers, `aggregate`, `render`, `run`.
- `crates/tokusage-cli/src/commands/mod.rs` — **modify**, declare `show`.
- `crates/tokusage-cli/src/main.rs` — **modify**, declare `mod collect;`, add `Show` subcommand + route.
- `README.md` — **modify**, document `tokusage show`.

Note: `SourceArg` (defined in `main.rs` crate root) is reachable from sibling modules as `crate::SourceArg` without any visibility change — descendant modules can see ancestor-module private items.

Note: Tasks 2–4 add code to `show.rs` before `run()` is wired in Task 5, so `cargo build` will emit `dead_code` warnings for the new functions until Task 5. That is expected; the build still succeeds and tests still run.

---

### Task 1: Extract `collect` into a shared module

Pure refactor — move the source-scan helpers out of `submit.rs` so `show` can reuse them. The existing test suite + compiler is the safety net (no new test).

**Files:**
- Create: `crates/tokusage-cli/src/collect.rs`
- Modify: `crates/tokusage-cli/src/commands/submit.rs:1-8` (imports) and `:148-189` (remove moved fns), `:16` (call site)
- Modify: `crates/tokusage-cli/src/main.rs:4-10` (add `mod collect;`)

- [ ] **Step 1: Create `collect.rs` with the moved orchestration**

Create `crates/tokusage-cli/src/collect.rs`:

```rust
use crate::SourceArg;
use anyhow::{Context, Result};
use tokusage_core::{sources, UnifiedMessage};

/// Scan one or all local sources into a flat list of usage messages.
/// With `source = None`, per-source failures are logged and skipped.
pub fn collect(source: Option<SourceArg>) -> Result<Vec<UnifiedMessage>> {
    match source {
        Some(SourceArg::Claude) => collect_claude(),
        Some(SourceArg::Codex) => collect_codex(),
        Some(SourceArg::Cursor) => collect_cursor(),
        None => {
            let mut out = Vec::new();
            match collect_claude() {
                Ok(mut v) => out.append(&mut v),
                Err(e) => tracing::warn!("claude source failed: {e}"),
            }
            match collect_codex() {
                Ok(mut v) => out.append(&mut v),
                Err(e) => tracing::warn!("codex source failed: {e}"),
            }
            match collect_cursor() {
                Ok(mut v) => out.append(&mut v),
                Err(e) => tracing::warn!("cursor source failed: {e}"),
            }
            Ok(out)
        }
    }
}

fn collect_claude() -> Result<Vec<UnifiedMessage>> {
    let root = sources::claude::default_root()
        .context("could not resolve home directory for Claude root")?;
    sources::claude::scan(&root)
}

fn collect_codex() -> Result<Vec<UnifiedMessage>> {
    let root =
        sources::codex::default_root().context("could not resolve Codex sessions directory")?;
    sources::codex::scan(&root)
}

fn collect_cursor() -> Result<Vec<UnifiedMessage>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(sources::cursor::scan())
}
```

- [ ] **Step 2: Declare the module in `main.rs`**

In `crates/tokusage-cli/src/main.rs`, the current module declarations are:

```rust
mod claude_hook;
mod commands;
mod config;
mod log_rotate;
mod manifest;
mod platform;
mod queue;
```

Add `mod collect;` after `mod claude_hook;`:

```rust
mod claude_hook;
mod collect;
mod commands;
mod config;
mod log_rotate;
mod manifest;
mod platform;
mod queue;
```

- [ ] **Step 3: Remove the moved functions from `submit.rs` and update the call site**

In `crates/tokusage-cli/src/commands/submit.rs`, delete the four functions `collect`, `collect_claude`, `collect_codex`, `collect_cursor` (currently lines 148–189, the block starting `fn collect(source: Option<SourceArg>) -> Result<Vec<UnifiedMessage>> {` and ending just before `#[cfg(test)]`).

Change the call site at line 16 from:

```rust
    let messages = collect(source)?;
```

to:

```rust
    let messages = crate::collect::collect(source)?;
```

Update the import on line 8 from:

```rust
use tokusage_core::{sources, SubmitEvent, SubmitPayload, UnifiedMessage};
```

to (drop the now-unused `sources`):

```rust
use tokusage_core::{SubmitEvent, SubmitPayload, UnifiedMessage};
```

Leave `use crate::SourceArg;` and `use anyhow::{Context, Result};` as they are — both are still used by `run`/`post_payload`.

- [ ] **Step 4: Build and run the existing tests**

Run: `cargo build -p tokusage-cli`
Expected: compiles cleanly (no errors).

Run: `cargo test -p tokusage-cli`
Expected: PASS — existing tests including `builds_raw_event_submit_request` still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-cli/src/collect.rs crates/tokusage-cli/src/commands/submit.rs crates/tokusage-cli/src/main.rs
git commit -m "refactor: extract source-scan collect into shared module

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Formatting helpers (`humanize`, `bar`, `sparkline`)

**Files:**
- Create: `crates/tokusage-cli/src/commands/show.rs`
- Modify: `crates/tokusage-cli/src/commands/mod.rs`

- [ ] **Step 1: Declare the module**

In `crates/tokusage-cli/src/commands/mod.rs`, add `pub mod show;` keeping the list alphabetical:

```rust
pub mod init;
pub mod login;
pub mod self_uninstall;
pub mod self_update;
pub mod show;
pub mod status;
pub mod submit;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/tokusage-cli/src/commands/show.rs` with only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_scales_to_k_and_m() {
        assert_eq!(humanize(0), "0");
        assert_eq!(humanize(999), "999");
        assert_eq!(humanize(1_000), "1.0K");
        assert_eq!(humanize(12_345), "12.3K");
        assert_eq!(humanize(2_400_000), "2.4M");
    }

    #[test]
    fn bar_normalizes_and_guards_zero_max() {
        assert_eq!(bar(0, 100, 12), "");
        assert_eq!(bar(10, 0, 12), ""); // zero max never divides
        assert_eq!(bar(50, 100, 10).chars().count(), 5);
        assert_eq!(bar(100, 100, 12).chars().count(), 12);
    }

    #[test]
    fn sparkline_handles_edges() {
        assert_eq!(sparkline(&[]), "");
        assert_eq!(sparkline(&[0, 0, 0]), "▁▁▁");
        assert_eq!(sparkline(&[5]), "█");
        assert_eq!(sparkline(&[0, 50, 100]), "▁▅█");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p tokusage-cli show`
Expected: FAIL to compile — `humanize`, `bar`, `sparkline` not found.

- [ ] **Step 4: Implement the helpers**

Add to the top of `crates/tokusage-cli/src/commands/show.rs` (above the `#[cfg(test)]` module):

```rust
/// Format a token count compactly: `999`, `1.0K`, `2.4M`.
fn humanize(n: i64) -> String {
    let v = n as f64;
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", v / 1_000.0)
    } else {
        format!("{:.1}M", v / 1_000_000.0)
    }
}

/// A horizontal bar of `█`, `value` scaled against `max` to at most `width`
/// cells. Returns empty when there is nothing to show.
fn bar(value: i64, max: i64, width: usize) -> String {
    if max <= 0 || value <= 0 {
        return String::new();
    }
    let filled = ((value as f64 / max as f64) * width as f64).round() as usize;
    "█".repeat(filled.clamp(0, width))
}

/// A one-line sparkline; each value scaled against the slice max into 8 levels.
fn sparkline(values: &[i64]) -> String {
    const TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if values.is_empty() {
        return String::new();
    }
    let max = *values.iter().max().unwrap_or(&0);
    values
        .iter()
        .map(|&v| {
            let idx = if max <= 0 {
                0
            } else {
                (((v.max(0) as f64) / max as f64) * (TICKS.len() - 1) as f64).round() as usize
            };
            TICKS[idx.min(TICKS.len() - 1)]
        })
        .collect()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p tokusage-cli show`
Expected: PASS — `humanize_scales_to_k_and_m`, `bar_normalizes_and_guards_zero_max`, `sparkline_handles_edges` all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/tokusage-cli/src/commands/show.rs crates/tokusage-cli/src/commands/mod.rs
git commit -m "feat: add show chart formatting helpers

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `aggregate` and report structs

**Files:**
- Modify: `crates/tokusage-cli/src/commands/show.rs`

- [ ] **Step 1: Write the failing test**

In `crates/tokusage-cli/src/commands/show.rs`, add inside the existing `mod tests` block. First make the test module imports available by replacing `use super::*;` with:

```rust
    use super::*;
    use chrono::{Local, TimeZone, Utc};
    use tokusage_core::{Client, TokenBreakdown, UnifiedMessage};

    fn msg(client: Client, ts: chrono::DateTime<Utc>, input: i64) -> UnifiedMessage {
        UnifiedMessage {
            client,
            event_key: "k".into(),
            session_key: None,
            seq: None,
            model: "m".into(),
            provider: "p".into(),
            timestamp: ts,
            tokens: TokenBreakdown {
                input,
                ..Default::default()
            },
            cost_cents: 0.0,
            raw_payload: serde_json::Value::Null,
        }
    }
```

Then add the test:

```rust
    #[test]
    fn aggregate_buckets_by_client_and_month() {
        // "now" = mid-June so day-of-month bucketing is timezone-stable.
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let messages = vec![
            msg(Client::Claude, Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap(), 100), // current
            msg(Client::Cursor, Utc.with_ymd_and_hms(2026, 6, 12, 12, 0, 0).unwrap(), 30),  // current
            msg(Client::Codex, Utc.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(), 70),   // last
            msg(Client::Claude, Utc.with_ymd_and_hms(2026, 4, 10, 12, 0, 0).unwrap(), 999), // excluded
        ];

        let report = aggregate(&messages, now);

        assert_eq!(report.current_label, "Jun");
        assert_eq!(report.last_label, "May");

        let claude = report.per_client.iter().find(|c| c.client == Client::Claude).unwrap();
        assert_eq!(claude.current.total(), 100); // April message excluded
        assert_eq!(claude.last.total(), 0);

        let codex = report.per_client.iter().find(|c| c.client == Client::Codex).unwrap();
        assert_eq!(codex.current.total(), 0);
        assert_eq!(codex.last.total(), 70);

        let cursor = report.per_client.iter().find(|c| c.client == Client::Cursor).unwrap();
        assert_eq!(cursor.current.total(), 30);

        // Daily series runs day 1..=today and sums to the current-month total.
        assert_eq!(report.daily_current.len(), 15);
        assert_eq!(report.daily_current.iter().sum::<i64>(), 130);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p tokusage-cli show::tests::aggregate_buckets_by_client_and_month`
Expected: FAIL to compile — `aggregate`, `Report`, `ClientMonths` not found.

- [ ] **Step 3: Implement structs and `aggregate`**

Add to the top of `crates/tokusage-cli/src/commands/show.rs` (above the helpers), and the imports at the very top of the file:

```rust
use chrono::{DateTime, Datelike, Local};
use tokusage_core::{Client, TokenBreakdown, UnifiedMessage};

const MON: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Per-client this-month / last-month token totals.
pub struct ClientMonths {
    pub client: Client,
    pub current: TokenBreakdown,
    pub last: TokenBreakdown,
}

/// Everything `render` needs to draw the chart.
pub struct Report {
    pub per_client: Vec<ClientMonths>,
    pub daily_current: Vec<i64>,
    pub current_label: String,
    pub last_label: String,
}

fn add(acc: &mut TokenBreakdown, t: &TokenBreakdown) {
    acc.input += t.input;
    acc.output += t.output;
    acc.cache_read += t.cache_read;
    acc.cache_write += t.cache_write;
    acc.reasoning += t.reasoning;
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    chrono::NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(31)
}

/// Bucket messages into per-client this-month/last-month totals plus a
/// current-month daily series (day 1..=today). Times are bucketed in local
/// time; `now` is injected for testability.
pub fn aggregate(messages: &[UnifiedMessage], now: DateTime<Local>) -> Report {
    let cur_y = now.year();
    let cur_m = now.month();
    let (last_y, last_m) = if cur_m == 1 {
        (cur_y - 1, 12)
    } else {
        (cur_y, cur_m - 1)
    };

    let order = [Client::Claude, Client::Codex, Client::Cursor];
    let mut per_client: Vec<ClientMonths> = order
        .iter()
        .map(|&c| ClientMonths {
            client: c,
            current: TokenBreakdown::default(),
            last: TokenBreakdown::default(),
        })
        .collect();

    let mut daily_current = vec![0i64; days_in_month(cur_y, cur_m) as usize];

    for m in messages {
        let local = m.timestamp.with_timezone(&Local);
        let (y, mo) = (local.year(), local.month());
        let slot = match per_client.iter_mut().find(|cm| cm.client == m.client) {
            Some(s) => s,
            None => continue,
        };
        if y == cur_y && mo == cur_m {
            add(&mut slot.current, &m.tokens);
            let d = local.day() as usize;
            if d >= 1 && d <= daily_current.len() {
                daily_current[d - 1] += m.tokens.total();
            }
        } else if y == last_y && mo == last_m {
            add(&mut slot.last, &m.tokens);
        }
    }

    daily_current.truncate(now.day() as usize);

    Report {
        per_client,
        daily_current,
        current_label: MON[(cur_m - 1) as usize].to_string(),
        last_label: MON[(last_m - 1) as usize].to_string(),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p tokusage-cli show::tests::aggregate_buckets_by_client_and_month`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-cli/src/commands/show.rs
git commit -m "feat: aggregate token usage by client and month

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `render`

**Files:**
- Modify: `crates/tokusage-cli/src/commands/show.rs`

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `crates/tokusage-cli/src/commands/show.rs`:

```rust
    fn tb(input: i64) -> TokenBreakdown {
        TokenBreakdown {
            input,
            ..Default::default()
        }
    }

    #[test]
    fn render_contains_key_lines() {
        let report = Report {
            per_client: vec![
                ClientMonths { client: Client::Claude, current: tb(2_400_000), last: tb(1_600_000) },
                ClientMonths { client: Client::Codex, current: tb(800_000), last: tb(1_100_000) },
                ClientMonths { client: Client::Cursor, current: tb(300_000), last: tb(200_000) },
            ],
            daily_current: vec![0, 1_000, 500_000, 900_000],
            current_label: "Jun".into(),
            last_label: "May".into(),
        };

        let s = render(&report);

        assert!(s.contains("Claude"));
        assert!(s.contains("Codex"));
        assert!(s.contains("Cursor"));
        assert!(s.contains("Jun"));
        assert!(s.contains("May"));
        assert!(s.contains("2.4M"));
        assert!(s.contains("Total"));
        assert!(s.contains("split:"));
        assert!(s.contains("Daily (Jun):"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p tokusage-cli show::tests::render_contains_key_lines`
Expected: FAIL to compile — `render` not found.

- [ ] **Step 3: Implement `render` and its small helpers**

Add to `crates/tokusage-cli/src/commands/show.rs` (above the `#[cfg(test)]` module):

```rust
const BAR_WIDTH: usize = 12;

fn client_name(c: Client) -> &'static str {
    match c {
        Client::Claude => "Claude",
        Client::Codex => "Codex",
        Client::Cursor => "Cursor",
    }
}

fn pct_change(cur: i64, last: i64) -> String {
    if last <= 0 {
        return "(—)".to_string();
    }
    let p = ((cur - last) as f64 / last as f64 * 100.0).round() as i64;
    if p >= 0 {
        format!("(+{}%)", p)
    } else {
        format!("({}%)", p)
    }
}

/// Render the full chart as a printable string (no trailing newline trimming).
pub fn render(report: &Report) -> String {
    let max = report
        .per_client
        .iter()
        .flat_map(|c| [c.current.total(), c.last.total()])
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    out.push_str("tokusage — token usage (本机)\n\n");

    for c in &report.per_client {
        let cur = c.current.total();
        let last = c.last.total();
        out.push_str(&format!(
            "{:<7} {} {:<wb$} {:>6}  {} {:<wb$} {:>6}\n",
            client_name(c.client),
            report.current_label,
            bar(cur, max, BAR_WIDTH),
            humanize(cur),
            report.last_label,
            bar(last, max, BAR_WIDTH),
            humanize(last),
            wb = BAR_WIDTH,
        ));
    }

    out.push('\n');
    out.push_str(&format!(
        "Daily ({}): {}\n",
        report.current_label,
        sparkline(&report.daily_current)
    ));

    let cur_total: i64 = report.per_client.iter().map(|c| c.current.total()).sum();
    let last_total: i64 = report.per_client.iter().map(|c| c.last.total()).sum();
    out.push_str("──────────────────────────────────\n");
    out.push_str(&format!(
        "Total {} {}  {} {}  {}\n",
        report.current_label,
        humanize(cur_total),
        report.last_label,
        humanize(last_total),
        pct_change(cur_total, last_total),
    ));

    let (in_, out_, cache) = report.per_client.iter().fold((0, 0, 0), |(i, o, c), cm| {
        (
            i + cm.current.input,
            o + cm.current.output,
            c + cm.current.cache_read + cm.current.cache_write,
        )
    });
    out.push_str(&format!(
        "{} split: in {} · out {} · cache {}\n",
        report.current_label,
        humanize(in_),
        humanize(out_),
        humanize(cache),
    ));

    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p tokusage-cli show::tests::render_contains_key_lines`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-cli/src/commands/show.rs
git commit -m "feat: render token usage chart string

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Wire up `run`, the `show` subcommand, and docs

**Files:**
- Modify: `crates/tokusage-cli/src/commands/show.rs` (add `run`)
- Modify: `crates/tokusage-cli/src/main.rs` (add `Show` variant + route)
- Modify: `README.md`

- [ ] **Step 1: Add `run` to `show.rs`**

Add to `crates/tokusage-cli/src/commands/show.rs` (above the `#[cfg(test)]` module). Also add `use anyhow::Result;` to the file's import block at the top:

```rust
pub fn run() -> Result<()> {
    let messages = crate::collect::collect(None)?;
    let report = aggregate(&messages, Local::now());

    let has_data = report
        .per_client
        .iter()
        .any(|c| c.current.total() > 0 || c.last.total() > 0);
    if !has_data {
        println!(
            "No local token usage found yet for {}–{}. Run your AI tools, then try again (or 'tokusage submit').",
            report.last_label, report.current_label
        );
        return Ok(());
    }

    println!("{}", render(&report));
    Ok(())
}
```

- [ ] **Step 2: Add the `Show` subcommand in `main.rs`**

In `crates/tokusage-cli/src/main.rs`, add this variant to the `Command` enum (after `Status,`):

```rust
    /// Show a local token usage chart for this and last month
    Show,
```

And add the match arm in `main()` (after the `Command::Status => ...` arm):

```rust
        Command::Show => commands::show::run(),
```

- [ ] **Step 3: Build and verify the whole workspace, including a manual smoke run**

Run: `cargo build -p tokusage-cli`
Expected: compiles with **no** `dead_code` warnings for `show` (everything is now reachable via `run`).

Run: `cargo test -p tokusage-cli`
Expected: PASS — all tests.

Run: `cargo run -p tokusage-cli -- show`
Expected: either the chart (if this machine has local usage) or the "No local token usage found yet for …" line. No JSON, no panic, exit 0.

- [ ] **Step 4: Document the command in the README**

In `README.md`, under the "## Ongoing" section, the current block is:

```bash
tokusage status       # show config, install state, queued retries, last run time
tokusage submit       # run once on demand
tokusage self-update  # fetch latest release and re-install
```

Change it to add the `show` line:

```bash
tokusage status       # show config, install state, queued retries, last run time
tokusage show         # local chart of this vs last month token usage (no network)
tokusage submit       # run once on demand
tokusage self-update  # fetch latest release and re-install
```

- [ ] **Step 5: Commit**

```bash
git add crates/tokusage-cli/src/commands/show.rs crates/tokusage-cli/src/main.rs README.md
git commit -m "feat: add tokusage show command

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Final Verification

- [ ] `cargo build` (whole workspace) succeeds with no warnings.
- [ ] `cargo test` (whole workspace) passes.
- [ ] `cargo run -p tokusage-cli -- show` prints a chart or the empty-state line — never raw JSON.
- [ ] `cargo run -p tokusage-cli -- submit --dry-run` still works (refactor regression check).
