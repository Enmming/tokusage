use anyhow::Result;
use chrono::{DateTime, Datelike, Local};
use tokusage_core::{Client, TokenBreakdown, UnifiedMessage};

/// Format a token count compactly: `999`, `1.0K`, `2.4M`, `1.1B`.
fn humanize(n: i64) -> String {
    let v = n as f64;
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", v / 1_000.0)
    } else if n < 1_000_000_000 {
        format!("{:.1}M", v / 1_000_000.0)
    } else {
        format!("{:.1}B", v / 1_000_000_000.0)
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
    out.push_str("tokusage — token usage (local)\n\n");

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

    print!("{}", render(&report));
    Ok(())
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn humanize_scales_to_k_m_and_b() {
        assert_eq!(humanize(0), "0");
        assert_eq!(humanize(999), "999");
        assert_eq!(humanize(1_000), "1.0K");
        assert_eq!(humanize(12_345), "12.3K");
        assert_eq!(humanize(2_400_000), "2.4M");
        assert_eq!(humanize(1_068_200_000), "1.1B");
        assert_eq!(humanize(2_202_000_000), "2.2B");
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

    fn tb(input: i64) -> TokenBreakdown {
        TokenBreakdown {
            input,
            ..Default::default()
        }
    }

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

    #[test]
    fn aggregate_handles_january_rollover() {
        // January's "last month" is December of the previous year.
        let now = Local.with_ymd_and_hms(2026, 1, 5, 12, 0, 0).unwrap();
        let messages = vec![
            msg(Client::Claude, Utc.with_ymd_and_hms(2026, 1, 3, 12, 0, 0).unwrap(), 50), // current (Jan)
            msg(Client::Codex, Utc.with_ymd_and_hms(2025, 12, 20, 12, 0, 0).unwrap(), 80), // last (Dec 2025)
        ];

        let report = aggregate(&messages, now);

        assert_eq!(report.current_label, "Jan");
        assert_eq!(report.last_label, "Dec");

        let claude = report.per_client.iter().find(|c| c.client == Client::Claude).unwrap();
        assert_eq!(claude.current.total(), 50);

        let codex = report.per_client.iter().find(|c| c.client == Client::Codex).unwrap();
        assert_eq!(codex.last.total(), 80);

        // Daily series is truncated to "today" (Jan 5).
        assert_eq!(report.daily_current.len(), 5);
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
}
