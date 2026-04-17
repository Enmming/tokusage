use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Claude,
    Codex,
    Cursor,
}

impl Client {
    pub fn as_str(&self) -> &'static str {
        match self {
            Client::Claude => "claude",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenBreakdown {
    #[serde(default)]
    pub input: i64,
    #[serde(default)]
    pub output: i64,
    #[serde(default)]
    pub cache_read: i64,
    #[serde(default)]
    pub cache_write: i64,
    #[serde(default)]
    pub reasoning: i64,
}

impl TokenBreakdown {
    pub fn total(&self) -> i64 {
        self.input + self.output + self.cache_read + self.cache_write + self.reasoning
    }
}

/// One unit of usage after parsing a source. Internal model, not for wire.
#[derive(Debug, Clone)]
pub struct UnifiedMessage {
    pub client: Client,
    pub model: String,
    pub provider: String,
    pub timestamp: DateTime<Utc>,
    pub tokens: TokenBreakdown,
    pub cost_cents: f64,
    pub dedup_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub generated_at: DateTime<Utc>,
    pub client_version: String,
    pub host_id: String,
    pub date_range: DateRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub date: NaiveDate,
    pub client: Client,
    pub model: String,
    pub provider: String,
    pub tokens: TokenBreakdown,
    pub cost_cents: f64,
    pub message_count: u32,
    pub dedup_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitPayload {
    pub meta: Meta,
    pub contributions: Vec<Contribution>,
}
