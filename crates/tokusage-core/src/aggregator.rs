//! Collapses a stream of UnifiedMessage into (date, client, model) groups,
//! deduping within the stream by UnifiedMessage.dedup_key so a server-side
//! UPSERT only has to deal with cross-submission dedup.

use crate::model::{Contribution, DateRange, Meta, SubmitPayload, TokenBreakdown, UnifiedMessage};
use chrono::{NaiveDate, Utc};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct GroupKey {
    date: NaiveDate,
    client: String,
    model: String,
    provider: String,
}

struct GroupAcc {
    tokens: TokenBreakdown,
    cost_cents: f64,
    message_count: u32,
    dedup_keys: Vec<String>,
}

pub fn build_payload(
    messages: Vec<UnifiedMessage>,
    client_version: &str,
    host_id: &str,
) -> SubmitPayload {
    let mut seen: HashSet<String> = HashSet::new();
    let mut groups: BTreeMap<GroupKey, GroupAcc> = BTreeMap::new();
    let mut min_date: Option<NaiveDate> = None;
    let mut max_date: Option<NaiveDate> = None;

    for m in messages {
        if !seen.insert(m.dedup_key.clone()) {
            continue;
        }
        let date = m.timestamp.date_naive();
        min_date = Some(min_date.map_or(date, |d| d.min(date)));
        max_date = Some(max_date.map_or(date, |d| d.max(date)));

        let key = GroupKey {
            date,
            client: m.client.as_str().to_string(),
            model: m.model.clone(),
            provider: m.provider.clone(),
        };
        let entry = groups.entry(key).or_insert_with(|| GroupAcc {
            tokens: TokenBreakdown::default(),
            cost_cents: 0.0,
            message_count: 0,
            dedup_keys: Vec::new(),
        });
        entry.tokens.input += m.tokens.input;
        entry.tokens.output += m.tokens.output;
        entry.tokens.cache_read += m.tokens.cache_read;
        entry.tokens.cache_write += m.tokens.cache_write;
        entry.tokens.reasoning += m.tokens.reasoning;
        entry.cost_cents += m.cost_cents;
        entry.message_count += 1;
        entry.dedup_keys.push(m.dedup_key);
    }

    let today = Utc::now().date_naive();
    let start = min_date.unwrap_or(today);
    let end = max_date.unwrap_or(today);

    let contributions = groups
        .into_iter()
        .map(|(k, v)| Contribution {
            date: k.date,
            client: match k.client.as_str() {
                "claude" => crate::model::Client::Claude,
                "codex" => crate::model::Client::Codex,
                "cursor" => crate::model::Client::Cursor,
                _ => crate::model::Client::Claude,
            },
            model: k.model,
            provider: k.provider,
            tokens: v.tokens,
            cost_cents: v.cost_cents,
            message_count: v.message_count,
            dedup_keys: v.dedup_keys,
        })
        .collect();

    SubmitPayload {
        meta: Meta {
            generated_at: Utc::now(),
            client_version: client_version.to_string(),
            host_id: host_id.to_string(),
            date_range: DateRange { start, end },
        },
        contributions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Client;
    use chrono::TimeZone;

    fn msg(client: Client, model: &str, ts_iso: &str, dedup: &str, input: i64) -> UnifiedMessage {
        UnifiedMessage {
            client,
            model: model.to_string(),
            provider: "test".to_string(),
            timestamp: chrono::DateTime::parse_from_rfc3339(ts_iso)
                .unwrap()
                .with_timezone(&Utc),
            tokens: TokenBreakdown {
                input,
                ..Default::default()
            },
            cost_cents: 0.0,
            dedup_key: dedup.to_string(),
        }
    }

    #[test]
    fn dedups_by_key_and_groups_by_date_model() {
        let msgs = vec![
            msg(Client::Claude, "opus", "2026-04-17T10:00:00Z", "k1", 10),
            msg(Client::Claude, "opus", "2026-04-17T11:00:00Z", "k2", 20),
            msg(Client::Claude, "opus", "2026-04-17T12:00:00Z", "k1", 99), // dup
            msg(Client::Claude, "sonnet", "2026-04-17T13:00:00Z", "k3", 5),
        ];
        let p = build_payload(msgs, "0.1.0", "host");
        assert_eq!(p.contributions.len(), 2);
        let opus = p
            .contributions
            .iter()
            .find(|c| c.model == "opus")
            .unwrap();
        assert_eq!(opus.tokens.input, 30); // 10 + 20, dup ignored
        assert_eq!(opus.message_count, 2);
    }

    #[test]
    fn empty_messages_produces_empty_payload_with_today_range() {
        let p = build_payload(vec![], "0.1.0", "host");
        assert_eq!(p.contributions.len(), 0);
        assert_eq!(p.meta.date_range.start, p.meta.date_range.end);
    }

    #[test]
    fn date_range_spans_min_to_max() {
        let msgs = vec![
            msg(Client::Claude, "opus", "2026-04-10T10:00:00Z", "k1", 1),
            msg(Client::Claude, "opus", "2026-04-17T10:00:00Z", "k2", 1),
            msg(Client::Claude, "opus", "2026-04-15T10:00:00Z", "k3", 1),
        ];
        let p = build_payload(msgs, "0.1.0", "host");
        assert_eq!(
            p.meta.date_range.start,
            Utc.with_ymd_and_hms(2026, 4, 10, 0, 0, 0)
                .unwrap()
                .date_naive()
        );
        assert_eq!(
            p.meta.date_range.end,
            Utc.with_ymd_and_hms(2026, 4, 17, 0, 0, 0)
                .unwrap()
                .date_naive()
        );
    }
}
