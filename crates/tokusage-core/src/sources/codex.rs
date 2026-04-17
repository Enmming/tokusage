use super::ScanResult;
use std::path::PathBuf;

pub fn default_root() -> Option<PathBuf> {
    if let Ok(var) = std::env::var("CODEX_HOME") {
        return Some(PathBuf::from(var).join("sessions"));
    }
    directories::BaseDirs::new().map(|d| d.home_dir().join(".codex/sessions"))
}

pub fn scan(_root: &std::path::Path) -> ScanResult {
    anyhow::bail!("codex source: not yet implemented (M2)")
}
