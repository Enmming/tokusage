use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Claude,
    Codex,
    Cursor,
    OpenCode,
}

impl Client {
    pub fn as_str(&self) -> &'static str {
        match self {
            Client::Claude => "claude",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
            Client::OpenCode => "opencode",
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
    pub event_key: String,
    pub session_key: Option<String>,
    pub seq: Option<u64>,
    pub model: String,
    pub provider: String,
    pub timestamp: DateTime<Utc>,
    pub tokens: TokenBreakdown,
    pub cost_cents: f64,
    pub raw_payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitEvent {
    pub source: Client,
    pub event_key: String,
    pub event_ts: DateTime<Utc>,
    pub session_key: Option<String>,
    pub seq: Option<u64>,
    pub model: String,
    pub provider: String,
    pub tokens: TokenBreakdown,
    pub cost_cents: f64,
    pub raw_payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitRequest {
    pub client_version: String,
    pub submitted_at: DateTime<Utc>,
    pub events: Vec<SubmitEvent>,
}

pub type SubmitPayload = SubmitRequest;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_client_is_lowercase_on_wire() {
        assert_eq!(Client::OpenCode.as_str(), "opencode");
        assert_eq!(
            serde_json::to_string(&Client::OpenCode).unwrap(),
            "\"opencode\""
        );
        let back: Client = serde_json::from_str("\"opencode\"").unwrap();
        assert_eq!(back, Client::OpenCode);
    }
}
