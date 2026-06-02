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
