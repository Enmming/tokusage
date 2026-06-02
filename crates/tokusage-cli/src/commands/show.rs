use chrono::{DateTime, Datelike, Local};
use tokusage_core::{Client, TokenBreakdown, UnifiedMessage};

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
}
