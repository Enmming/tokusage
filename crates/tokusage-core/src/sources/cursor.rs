//! Cursor IDE usage collector.
//!
//! Cursor does **not** write usage data to disk — the IDE pulls it from the
//! backend each time the dashboard is opened. We replicate that call:
//!
//! 1. Read the Cursor IDE's own access JWT from its SQLite key-value store
//!    (`cursorAuth/accessToken`). The token is stored in plaintext because
//!    the IDE is an Electron app and just keeps it as a regular value.
//! 2. POST to `api2.cursor.sh/aiserver.v1.DashboardService/GetFilteredUsageEvents`
//!    with `Authorization: Bearer <jwt>`. That returns the same per-event
//!    stream Cursor shows in the web dashboard — already priced.
//!
//! The base URL is injectable so unit tests can stub the RPC via mockito.

use super::ScanResult;
use crate::model::{Client, TokenBreakdown, UnifiedMessage};
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::path::{Path, PathBuf};

const DEFAULT_API_BASE: &str = "https://api2.cursor.sh";
const RPC_PATH: &str = "/aiserver.v1.DashboardService/GetFilteredUsageEvents";

/// Max events the Cursor RPC returns per call (probed 2026-04-17: 1000 works,
/// 2000+ silently returns 0).
const PAGE_SIZE: u32 = 1000;
/// Safety cap on total pages to prevent a runaway loop if Cursor changes
/// behavior. 20 pages * 1000 = 20k events, way more than any single user.
const MAX_PAGES: u32 = 20;

pub fn default_db_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| {
        d.home_dir()
            .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    })
}

/// Read the JWT Cursor IDE stores for its own backend auth.
pub fn read_jwt(db_path: &Path) -> anyhow::Result<String> {
    if !db_path.exists() {
        anyhow::bail!(
            "Cursor state DB not found at {}. Is Cursor IDE installed and logged in?",
            db_path.display()
        );
    }
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let jwt: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key='cursorAuth/accessToken'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "could not read cursorAuth/accessToken from {}: {e}. Try signing into Cursor IDE again.",
                db_path.display()
            )
        })?;
    if jwt.trim().is_empty() {
        anyhow::bail!("Cursor access token is empty. Sign into Cursor IDE first.");
    }
    Ok(jwt)
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    #[serde(rename = "usageEventsDisplay", default)]
    events: Vec<RpcEvent>,
}

#[derive(Debug, Deserialize)]
struct RpcEvent {
    /// Epoch millis, serialized as string (protobuf int64 convention).
    timestamp: String,
    model: String,
    #[serde(default)]
    #[allow(dead_code)]
    kind: String,
    #[serde(rename = "tokenUsage")]
    token_usage: Option<RpcTokenUsage>,
    #[serde(rename = "owningUser", default)]
    owning_user: String,
    #[serde(rename = "isHeadless", default)]
    is_headless: bool,
}

#[derive(Debug, Deserialize, Default)]
struct RpcTokenUsage {
    #[serde(rename = "inputTokens", default)]
    input: i64,
    #[serde(rename = "outputTokens", default)]
    output: i64,
    #[serde(rename = "cacheReadTokens", default)]
    cache_read: i64,
    #[serde(rename = "cacheWriteTokens", default)]
    cache_write: i64,
    #[serde(rename = "totalCents", default)]
    total_cents: f64,
}

/// Build the default HTTP client for talking to Cursor. Respects the user's
/// HTTP(S)_PROXY env vars so employees behind a corporate proxy still work.
fn default_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

/// Error returned by `fetch_events`. Split out from `anyhow::Error` so that
/// callers can react to `Unauthorized` by re-reading the JWT (Cursor IDE may
/// have refreshed it in the meantime) and retrying.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("Cursor JWT rejected ({status}). Reopen Cursor IDE and try again.")]
    Unauthorized { status: reqwest::StatusCode },
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl From<reqwest::Error> for FetchError {
    fn from(e: reqwest::Error) -> Self {
        FetchError::Other(e.into())
    }
}

/// Call Cursor's GetFilteredUsageEvents RPC, paginating until we've drained
/// every event or hit MAX_PAGES. `api_base` should include scheme + host,
/// e.g. `https://api2.cursor.sh` (or a mockito URL in tests).
pub async fn fetch_events(
    client: &reqwest::Client,
    jwt: &str,
    api_base: &str,
) -> Result<Vec<UnifiedMessage>, FetchError> {
    let mut all = Vec::new();
    for page in 1..=MAX_PAGES {
        let events = fetch_page(client, jwt, api_base, page, PAGE_SIZE).await?;
        let count = events.len();
        all.extend(events);
        // Cursor returns fewer than pageSize items when we hit the end.
        if count < PAGE_SIZE as usize {
            break;
        }
    }
    Ok(all.into_iter().filter_map(event_to_unified).collect())
}

async fn fetch_page(
    client: &reqwest::Client,
    jwt: &str,
    api_base: &str,
    page: u32,
    page_size: u32,
) -> Result<Vec<RpcEvent>, FetchError> {
    let url = format!("{}{}", api_base, RPC_PATH);
    let body = serde_json::json!({ "page": page, "pageSize": page_size }).to_string();

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .body(body)
        .send()
        .await?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(FetchError::Unauthorized { status });
    }

    let body_text = resp.text().await?;

    if !status.is_success() {
        return Err(FetchError::Other(anyhow::anyhow!(
            "Cursor RPC returned {} on page {}: {}",
            status,
            page,
            truncate(&body_text, 400)
        )));
    }

    let parsed: RpcResponse = serde_json::from_str(&body_text).map_err(|e| {
        FetchError::Other(anyhow::anyhow!(
            "could not parse Cursor RPC response on page {page}: {e}. Body starts with: {}",
            truncate(&body_text, 200)
        ))
    })?;

    Ok(parsed.events)
}

fn event_to_unified(ev: RpcEvent) -> Option<UnifiedMessage> {
    let millis: i64 = ev.timestamp.parse().ok()?;
    let timestamp = Utc.timestamp_millis_opt(millis).single()?;
    let usage = ev.token_usage.unwrap_or_default();

    let dedup_key = format!(
        "cursor:{}:{}:{}:{}",
        ev.timestamp,
        ev.owning_user,
        ev.model,
        if ev.is_headless { "hl" } else { "ui" }
    );

    Some(UnifiedMessage {
        client: Client::Cursor,
        model: ev.model,
        provider: "cursor".to_string(),
        timestamp,
        tokens: TokenBreakdown {
            input: usage.input,
            output: usage.output,
            cache_read: usage.cache_read,
            cache_write: usage.cache_write,
            reasoning: 0,
        },
        cost_cents: usage.total_cents,
        dedup_key,
    })
}

/// Default scan: read the JWT from the default DB location and call the real
/// Cursor API. If the first call returns 401/403, re-read the JWT once —
/// Cursor IDE rotates its access token silently, and the value we read 30
/// seconds ago may already be stale.
pub async fn scan() -> ScanResult {
    let Some(db) = default_db_path() else {
        anyhow::bail!("could not resolve Cursor state DB path (no home directory)")
    };
    let client = default_client()?;

    let jwt = read_jwt(&db)?;
    match fetch_events(&client, &jwt, DEFAULT_API_BASE).await {
        Ok(msgs) => Ok(msgs),
        Err(FetchError::Unauthorized { .. }) => {
            tracing::info!("Cursor JWT rejected; re-reading from SQLite and retrying once");
            let fresh_jwt = read_jwt(&db)?;
            if fresh_jwt == jwt {
                anyhow::bail!(
                    "Cursor JWT rejected and no newer token available in Cursor IDE's state DB. \
                     Open Cursor IDE and sign in again, then re-run."
                );
            }
            match fetch_events(&client, &fresh_jwt, DEFAULT_API_BASE).await {
                Ok(msgs) => Ok(msgs),
                Err(e) => Err(e.into()),
            }
        }
        Err(FetchError::Other(e)) => Err(e),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn make_cursor_db() -> NamedTempFile {
        let tmp = NamedTempFile::new().unwrap();
        let conn = rusqlite::Connection::open(tmp.path()).unwrap();
        conn.execute("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)", [])
            .unwrap();
        tmp
    }

    #[test]
    fn reads_jwt_from_sqlite() {
        let tmp = make_cursor_db();
        {
            let conn = rusqlite::Connection::open(tmp.path()).unwrap();
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('cursorAuth/accessToken', 'eyJ.sample.jwt')",
                [],
            )
            .unwrap();
        }
        let jwt = read_jwt(tmp.path()).unwrap();
        assert_eq!(jwt, "eyJ.sample.jwt");
    }

    #[test]
    fn read_jwt_fails_on_missing_row() {
        let tmp = make_cursor_db();
        let err = read_jwt(tmp.path()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("cursorAuth/accessToken"), "got: {msg}");
    }

    #[test]
    fn read_jwt_fails_on_missing_file() {
        let err = read_jwt(Path::new("/nonexistent/state.vscdb")).unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[tokio::test]
    async fn fetch_events_parses_real_shape() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", RPC_PATH)
            .match_header("authorization", "Bearer jwt-xyz")
            .match_header("content-type", "application/json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                  "totalUsageEventsCount": 2,
                  "usageEventsDisplay": [
                    {
                      "timestamp": "1776348340274",
                      "model": "gpt-5.4-medium",
                      "kind": "USAGE_EVENT_KIND_INCLUDED_IN_PRO",
                      "tokenUsage": {
                        "inputTokens": 1080,
                        "outputTokens": 390,
                        "cacheReadTokens": 23552,
                        "totalCents": 0.9578
                      },
                      "owningUser": "234376495",
                      "isHeadless": false
                    },
                    {
                      "timestamp": "1776348500000",
                      "model": "claude-4.6-sonnet",
                      "kind": "USAGE_EVENT_KIND_USAGE_BASED",
                      "tokenUsage": {
                        "inputTokens": 500,
                        "outputTokens": 100,
                        "cacheWriteTokens": 200,
                        "totalCents": 1.5
                      },
                      "owningUser": "234376495",
                      "isHeadless": true
                    }
                  ]
                }"#,
            )
            .create_async()
            .await;

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let msgs = fetch_events(&client, "jwt-xyz", &server.url()).await.unwrap();
        mock.assert_async().await;
        assert_eq!(msgs.len(), 2);

        let first = &msgs[0];
        assert_eq!(first.client, Client::Cursor);
        assert_eq!(first.model, "gpt-5.4-medium");
        assert_eq!(first.tokens.input, 1080);
        assert_eq!(first.tokens.cache_read, 23552);
        assert!((first.cost_cents - 0.9578).abs() < 1e-9);
        assert_eq!(
            first.dedup_key,
            "cursor:1776348340274:234376495:gpt-5.4-medium:ui"
        );

        let second = &msgs[1];
        assert_eq!(second.tokens.cache_write, 200);
        assert!(second.dedup_key.ends_with(":hl"));
    }

    #[tokio::test]
    async fn fetch_events_surfaces_auth_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", RPC_PATH)
            .with_status(401)
            .with_body(r#"{"error":"unauthenticated"}"#)
            .create_async()
            .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let err = fetch_events(&client, "bad", &server.url()).await.unwrap_err();
        assert!(matches!(err, FetchError::Unauthorized { .. }), "got: {err:?}");
    }

    #[tokio::test]
    async fn fetch_events_handles_empty_event_list() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", RPC_PATH)
            .with_status(200)
            .with_body(r#"{"totalUsageEventsCount": 0, "usageEventsDisplay": []}"#)
            .create_async()
            .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let msgs = fetch_events(&client, "jwt", &server.url()).await.unwrap();
        assert!(msgs.is_empty());
    }

    fn make_page(count: usize, start_ts: u64) -> String {
        let events: Vec<String> = (0..count)
            .map(|i| {
                format!(
                    r#"{{"timestamp":"{}","model":"gpt-5","tokenUsage":{{"inputTokens":1,"outputTokens":1}},"owningUser":"u"}}"#,
                    start_ts + i as u64
                )
            })
            .collect();
        format!(
            r#"{{"totalUsageEventsCount":{},"usageEventsDisplay":[{}]}}"#,
            count,
            events.join(",")
        )
    }

    #[tokio::test]
    async fn fetch_events_paginates_until_short_page() {
        // Page 1 returns PAGE_SIZE (full page) → loop continues.
        // Page 2 returns fewer → loop stops.
        let mut server = mockito::Server::new_async().await;
        let page1_body = make_page(PAGE_SIZE as usize, 1_700_000_000_000);
        let page2_body = make_page(50, 1_700_000_001_000);

        let m1 = server
            .mock("POST", RPC_PATH)
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"page":1}"#.to_string(),
            ))
            .with_status(200)
            .with_body(page1_body)
            .create_async()
            .await;
        let m2 = server
            .mock("POST", RPC_PATH)
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"page":2}"#.to_string(),
            ))
            .with_status(200)
            .with_body(page2_body)
            .create_async()
            .await;

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let msgs = fetch_events(&client, "jwt", &server.url()).await.unwrap();
        m1.assert_async().await;
        m2.assert_async().await;
        assert_eq!(msgs.len(), PAGE_SIZE as usize + 50);
    }
}
