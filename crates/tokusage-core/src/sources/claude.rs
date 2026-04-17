use super::ScanResult;
use std::path::PathBuf;

pub fn default_root() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".claude/projects"))
}

pub fn scan(_root: &std::path::Path) -> ScanResult {
    anyhow::bail!("claude source: not yet implemented (M1)")
}
