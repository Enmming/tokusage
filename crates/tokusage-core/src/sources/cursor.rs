use super::ScanResult;
use std::path::PathBuf;

pub fn default_db_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| {
        d.home_dir()
            .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    })
}

pub async fn scan() -> ScanResult {
    anyhow::bail!("cursor source: not yet implemented (M3)")
}
