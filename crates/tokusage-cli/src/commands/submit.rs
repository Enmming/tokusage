use crate::commands::login;
use crate::log_rotate;
use crate::manifest;
use crate::queue;
use crate::SourceArg;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokusage_core::{sources, SubmitEvent, SubmitPayload, UnifiedMessage};

pub fn run(dry_run: bool, source: Option<SourceArg>) -> Result<()> {
    // Opportunistic log rotation every run (cheap stat call if file fits).
    if let Ok(log) = manifest::log_path() {
        log_rotate::rotate_if_needed(&log);
    }

    let messages = collect(source)?;
    let payload = build_submit_request(messages, env!("CARGO_PKG_VERSION"), Utc::now());

    if dry_run {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let (api_url, api_token) = login::require_credentials()?;

    // Drain any queued payloads first (oldest first).
    drain_queue(&api_url, &api_token)?;

    if payload.events.is_empty() {
        tracing::info!("no usage data collected; nothing to submit");
        return Ok(());
    }

    if let Err(e) = post_payload(&api_url, &api_token, &payload) {
        tracing::warn!("submit failed, queuing for retry: {e}");
        queue::enqueue(&payload)?;
        return Err(e);
    }

    tracing::info!(events = payload.events.len(), "submit ok");
    Ok(())
}

fn build_submit_request(
    messages: Vec<UnifiedMessage>,
    client_version: &str,
    submitted_at: DateTime<Utc>,
) -> SubmitPayload {
    SubmitPayload {
        client_version: client_version.to_string(),
        submitted_at,
        events: messages
            .into_iter()
            .map(|message| SubmitEvent {
                source: message.client,
                event_key: message.event_key,
                event_ts: message.timestamp,
                session_key: message.session_key,
                seq: message.seq,
                model: message.model,
                provider: message.provider,
                tokens: message.tokens,
                cost_cents: message.cost_cents,
                raw_payload: message.raw_payload,
            })
            .collect(),
    }
}

fn drain_queue(api_url: &str, api_token: &str) -> Result<()> {
    let queued = queue::list()?;
    if queued.is_empty() {
        return Ok(());
    }
    tracing::info!(count = queued.len(), "draining queued submits");
    for path in queued {
        let payload = match queue::load(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(?path, "corrupt queue file; quarantining: {e}");
                queue::quarantine(&path).ok();
                continue;
            }
        };
        match post_payload(api_url, api_token, &payload) {
            Ok(_) => {
                queue::remove(&path).ok();
                tracing::info!(?path, "re-submitted queued payload");
            }
            Err(e) => {
                tracing::warn!(
                    ?path,
                    "queued re-submit still failing; will retry later: {e}"
                );
                // Stop draining; don't hammer a dead endpoint.
                return Ok(());
            }
        }
    }
    Ok(())
}

fn post_payload(api_url: &str, api_token: &str, payload: &SubmitPayload) -> Result<()> {
    let url = format!("{}/api/submit", api_url.trim_end_matches('/'));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));
        // Skip any system proxy for loopback. Corporate proxies routinely break
        // connections to 127.0.0.1 and talking to a local mock/dev server
        // should never go through a proxy anyway.
        if is_loopback(&url) {
            builder = builder.no_proxy();
        }
        let client = builder.build()?;
        let resp = client
            .post(&url)
            .bearer_auth(api_token)
            .json(payload)
            .send()
            .await
            .context("POST /api/submit")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("server returned {}: {}", status, truncate(&body, 400));
        }
        Ok(())
    })
}

fn is_loopback(url: &str) -> bool {
    url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
        || url.starts_with("http://[::1]")
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

fn collect(source: Option<SourceArg>) -> Result<Vec<UnifiedMessage>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use tokusage_core::{Client, SubmitRequest, TokenBreakdown};

    fn msg() -> UnifiedMessage {
        UnifiedMessage {
            client: Client::Claude,
            event_key: "claude:req_A:msg_1".to_string(),
            session_key: Some("claude:sha256:session".to_string()),
            seq: Some(3),
            model: "claude-opus-4-7".to_string(),
            provider: "anthropic".to_string(),
            timestamp: Utc.with_ymd_and_hms(2026, 4, 23, 10, 28, 41).unwrap(),
            tokens: TokenBreakdown {
                input: 6,
                output: 197,
                cache_read: 16757,
                cache_write: 10792,
                reasoning: 0,
            },
            cost_cents: 0.0,
            raw_payload: serde_json::json!({
                "request_id": "req_A",
                "message_id": "msg_1"
            }),
        }
    }

    #[test]
    fn builds_raw_event_submit_request() {
        let submitted_at = Utc.with_ymd_and_hms(2026, 4, 23, 10, 30, 0).unwrap();
        let payload: SubmitRequest = build_submit_request(vec![msg()], "0.2.0", submitted_at);

        assert_eq!(payload.client_version, "0.2.0");
        assert_eq!(payload.submitted_at, submitted_at);
        assert_eq!(payload.events.len(), 1);

        let event = &payload.events[0];
        assert_eq!(event.source, Client::Claude);
        assert_eq!(event.event_key, "claude:req_A:msg_1");
        assert_eq!(
            event.event_ts,
            Utc.with_ymd_and_hms(2026, 4, 23, 10, 28, 41).unwrap()
        );
        assert_eq!(event.session_key.as_deref(), Some("claude:sha256:session"));
        assert_eq!(event.seq, Some(3));
        assert_eq!(event.raw_payload["request_id"], "req_A");
    }
}
