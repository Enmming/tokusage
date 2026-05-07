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
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_API_BASE: &str = "https://api2.cursor.sh";
const RPC_PATH: &str = "/aiserver.v1.DashboardService/GetFilteredUsageEvents";
const OAUTH_TOKEN_PATH: &str = "/oauth/token";
const DEFAULT_AUTH_CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";

/// Max events the Cursor RPC returns per call (probed 2026-04-17: 1000 works,
/// 2000+ silently returns 0).
const PAGE_SIZE: u32 = 1000;
/// Safety cap on total pages to prevent a runaway loop if Cursor changes
/// behavior. 20 pages * 1000 = 20k events, way more than any single user.
const MAX_PAGES: u32 = 20;

fn api_base() -> String {
    std::env::var("TOKUSAGE_CURSOR_API_BASE").unwrap_or_else(|_| DEFAULT_API_BASE.to_string())
}

fn auth_client_id() -> String {
    std::env::var("TOKUSAGE_CURSOR_AUTH_CLIENT_ID")
        .unwrap_or_else(|_| DEFAULT_AUTH_CLIENT_ID.to_string())
}

fn use_proxy() -> bool {
    matches!(
        std::env::var("TOKUSAGE_CURSOR_USE_PROXY").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

pub fn default_db_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| {
        #[cfg(target_os = "macos")]
        {
            d.home_dir()
                .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
        }
        #[cfg(target_os = "linux")]
        {
            d.config_dir().join("Cursor/User/globalStorage/state.vscdb")
        }
        #[cfg(target_os = "windows")]
        {
            d.config_dir()
                .join("Cursor")
                .join("User")
                .join("globalStorage")
                .join("state.vscdb")
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            d.home_dir().join(".cursor/User/globalStorage/state.vscdb")
        }
    })
}

/// Read the JWT Cursor IDE stores for its own backend auth.
pub fn read_jwt(db_path: &Path) -> anyhow::Result<String> {
    read_token(db_path, "cursorAuth/accessToken", "Cursor access token")
}

pub fn read_refresh_token(db_path: &Path) -> anyhow::Result<String> {
    read_token(db_path, "cursorAuth/refreshToken", "Cursor refresh token")
}

fn read_token(db_path: &Path, key: &str, label: &str) -> anyhow::Result<String> {
    if !db_path.exists() {
        anyhow::bail!(
            "Cursor state DB not found at {}. Is Cursor IDE installed and logged in?",
            db_path.display()
        );
    }
    let conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let jwt: String = conn
        .query_row("SELECT value FROM ItemTable WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .map_err(|e| {
            anyhow::anyhow!(
                "could not read {key} from {}: {e}. Try signing into Cursor IDE again.",
                db_path.display()
            )
        })?;
    if jwt.trim().is_empty() {
        anyhow::bail!("{label} is empty. Sign into Cursor IDE first.");
    }
    Ok(jwt)
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    #[serde(rename = "usageEventsDisplay", default)]
    events: Vec<RpcEvent>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RpcEvent {
    /// Epoch millis, serialized as string (protobuf int64 convention).
    timestamp: String,
    model: String,
    #[serde(default)]
    kind: String,
    #[serde(rename = "tokenUsage")]
    token_usage: Option<RpcTokenUsage>,
    #[serde(rename = "owningUser", default)]
    owning_user: String,
    #[serde(rename = "isHeadless", default)]
    is_headless: bool,
}

#[derive(Debug, Deserialize, Default, Serialize)]
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

#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    access_token: Option<String>,
    #[allow(dead_code)]
    refresh_token: Option<String>,
    #[serde(rename = "shouldLogout")]
    should_logout: Option<bool>,
}

/// Build the default HTTP client for talking to Cursor.
///
/// In practice `reqwest + rustls` frequently fails on local HTTP(S)_PROXY
/// setups for Cursor's RPC while direct connections still succeed. We
/// therefore bypass proxies by default and let callers opt back into proxy
/// routing with `TOKUSAGE_CURSOR_USE_PROXY=1`.
fn default_client() -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));
    if !use_proxy() {
        builder = builder.no_proxy();
    }
    Ok(builder.build()?)
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

async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
    api_base: &str,
    auth_client_id: &str,
) -> Result<String, FetchError> {
    let url = format!("{}{}", api_base, OAUTH_TOKEN_PATH);
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": auth_client_id,
            "refresh_token": refresh_token,
        }))
        .send()
        .await?;
    let status = resp.status();
    let body_text = resp.text().await?;
    if !status.is_success() {
        return Err(FetchError::Other(anyhow::anyhow!(
            "Cursor token refresh returned {}: {}",
            status,
            truncate(&body_text, 400)
        )));
    }
    let parsed: RefreshTokenResponse = serde_json::from_str(&body_text).map_err(|e| {
        FetchError::Other(anyhow::anyhow!(
            "could not parse Cursor token refresh response: {e}. Body starts with: {}",
            truncate(&body_text, 200)
        ))
    })?;
    if parsed.should_logout == Some(true) {
        return Err(FetchError::Other(anyhow::anyhow!(
            "Cursor refresh endpoint requested logout; reopen Cursor IDE and sign in again."
        )));
    }
    let Some(access_token) = parsed.access_token.filter(|token| !token.trim().is_empty()) else {
        return Err(FetchError::Other(anyhow::anyhow!(
            "Cursor refresh response did not include a usable access_token."
        )));
    };
    Ok(access_token)
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
    let raw_payload = serde_json::to_value(&ev).ok()?;
    let usage = ev.token_usage.unwrap_or_default();
    let event_key = format!(
        "cursor:{}:{}:{}:{}:{}",
        ev.timestamp,
        ev.owning_user,
        ev.model,
        ev.kind,
        if ev.is_headless { "headless" } else { "ui" }
    );

    Some(UnifiedMessage {
        client: Client::Cursor,
        event_key,
        session_key: None,
        seq: None,
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
        raw_payload,
    })
}

/// Default scan: read the JWT from the default DB location and call the real
/// Cursor API. If the first call returns 401/403, re-read the JWT once —
/// Cursor IDE rotates its access token silently, and the value we read 30
/// seconds ago may already be stale. If the DB still has the old token, fall
/// back to the stored refresh token and exchange it for a fresh access token.
pub async fn scan() -> ScanResult {
    let Some(db) = default_db_path() else {
        anyhow::bail!("could not resolve Cursor state DB path (no home directory)")
    };
    let client = default_client()?;
    let api_base = api_base();
    let auth_client_id = auth_client_id();

    scan_with_client(&db, &client, &api_base, &auth_client_id).await
}

async fn scan_with_client(
    db_path: &Path,
    client: &reqwest::Client,
    api_base: &str,
    auth_client_id: &str,
) -> ScanResult {
    let jwt = read_jwt(db_path)?;
    match fetch_events(client, &jwt, api_base).await {
        Ok(msgs) => Ok(msgs),
        Err(FetchError::Unauthorized { .. }) => {
            tracing::info!("Cursor JWT rejected; re-reading from SQLite and retrying once");
            let fresh_jwt = read_jwt(db_path)?;
            if fresh_jwt != jwt {
                match fetch_events(client, &fresh_jwt, api_base).await {
                    Ok(msgs) => return Ok(msgs),
                    Err(FetchError::Unauthorized { .. }) => {
                        tracing::info!(
                            "Cursor DB still had a rejected access token; trying refresh token"
                        );
                    }
                    Err(FetchError::Other(e)) => return Err(e),
                }
            }
            let refresh_token = read_refresh_token(db_path)?;
            let refreshed_access =
                refresh_access_token(client, &refresh_token, api_base, auth_client_id).await?;
            match fetch_events(client, &refreshed_access, api_base).await {
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
        conn.execute(
            "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
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
    fn reads_refresh_token_from_sqlite() {
        let tmp = make_cursor_db();
        {
            let conn = rusqlite::Connection::open(tmp.path()).unwrap();
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('cursorAuth/refreshToken', 'refresh.jwt')",
                [],
            )
            .unwrap();
        }
        let refresh = read_refresh_token(tmp.path()).unwrap();
        assert_eq!(refresh, "refresh.jwt");
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
        let msgs = fetch_events(&client, "jwt-xyz", &server.url())
            .await
            .unwrap();
        mock.assert_async().await;
        assert_eq!(msgs.len(), 2);

        let first = &msgs[0];
        assert_eq!(first.client, Client::Cursor);
        assert_eq!(first.model, "gpt-5.4-medium");
        assert_eq!(first.tokens.input, 1080);
        assert_eq!(first.tokens.cache_read, 23552);
        assert!((first.cost_cents - 0.9578).abs() < 1e-9);
        assert_eq!(
            first.event_key,
            "cursor:1776348340274:234376495:gpt-5.4-medium:USAGE_EVENT_KIND_INCLUDED_IN_PRO:ui"
        );
        assert!(first.session_key.is_none());
        assert!(first.seq.is_none());
        assert_eq!(
            first.raw_payload["owningUser"],
            serde_json::Value::String("234376495".to_string())
        );
        assert_eq!(
            first.raw_payload["kind"],
            serde_json::Value::String("USAGE_EVENT_KIND_INCLUDED_IN_PRO".to_string())
        );
        assert_eq!(
            first.raw_payload["isHeadless"],
            serde_json::Value::Bool(false)
        );

        let second = &msgs[1];
        assert_eq!(second.tokens.cache_write, 200);
        assert_eq!(
            second.event_key,
            "cursor:1776348500000:234376495:claude-4.6-sonnet:USAGE_EVENT_KIND_USAGE_BASED:headless"
        );
        assert_eq!(second.raw_payload["tokenUsage"]["cacheWriteTokens"], 200);
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
        let err = fetch_events(&client, "bad", &server.url())
            .await
            .unwrap_err();
        assert!(
            matches!(err, FetchError::Unauthorized { .. }),
            "got: {err:?}"
        );
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

    #[tokio::test]
    async fn scan_refreshes_access_token_after_unauthorized() {
        let tmp = make_cursor_db();
        {
            let conn = rusqlite::Connection::open(tmp.path()).unwrap();
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('cursorAuth/accessToken', 'expired-access')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('cursorAuth/refreshToken', 'refresh-123')",
                [],
            )
            .unwrap();
        }

        let mut server = mockito::Server::new_async().await;
        let unauthorized = server
            .mock("POST", RPC_PATH)
            .match_header("authorization", "Bearer expired-access")
            .with_status(401)
            .with_body(r#"{"error":"unauthenticated"}"#)
            .create_async()
            .await;
        let refresh = server
            .mock("POST", "/oauth/token")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"grant_type":"refresh_token","client_id":"cursor-client","refresh_token":"refresh-123"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"access_token":"fresh-access","refresh_token":"fresh-refresh"}"#,
            )
            .create_async()
            .await;
        let usage = server
            .mock("POST", RPC_PATH)
            .match_header("authorization", "Bearer fresh-access")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"totalUsageEventsCount":1,"usageEventsDisplay":[{"timestamp":"1776348340274","model":"gpt-5.4-medium","kind":"USAGE_EVENT_KIND_INCLUDED_IN_PRO","tokenUsage":{"inputTokens":1080,"outputTokens":390,"cacheReadTokens":23552,"totalCents":0.9578},"owningUser":"234376495","isHeadless":false}]}"#,
            )
            .create_async()
            .await;

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let msgs = scan_with_client(tmp.path(), &client, &server.url(), "cursor-client")
            .await
            .unwrap();

        unauthorized.assert_async().await;
        refresh.assert_async().await;
        usage.assert_async().await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            msgs[0].event_key,
            "cursor:1776348340274:234376495:gpt-5.4-medium:USAGE_EVENT_KIND_INCLUDED_IN_PRO:ui"
        );
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

    #[test]
    fn api_base_prefers_env_override() {
        let key = "TOKUSAGE_CURSOR_API_BASE";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "http://127.0.0.1:18080");

        assert_eq!(api_base(), "http://127.0.0.1:18080");

        if let Some(value) = previous {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn use_proxy_defaults_false() {
        let key = "TOKUSAGE_CURSOR_USE_PROXY";
        let previous = std::env::var(key).ok();
        std::env::remove_var(key);

        assert!(!use_proxy());

        if let Some(value) = previous {
            std::env::set_var(key, value);
        }
    }

    #[test]
    fn use_proxy_respects_env_override() {
        let key = "TOKUSAGE_CURSOR_USE_PROXY";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "1");

        assert!(use_proxy());

        if let Some(value) = previous {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
