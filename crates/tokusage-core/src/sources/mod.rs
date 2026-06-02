pub mod claude;
pub mod codex;
pub mod cursor;
pub mod opencode;

use crate::UnifiedMessage;

/// Result of scanning a source. Empty vec means "source is healthy but has no data yet".
pub type ScanResult = anyhow::Result<Vec<UnifiedMessage>>;
