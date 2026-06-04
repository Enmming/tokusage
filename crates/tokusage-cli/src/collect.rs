use crate::SourceArg;
use anyhow::{Context, Result};
use tokusage_core::{sources, UnifiedMessage};

/// Scan one or all local sources into a flat list of usage messages.
/// With `source = None`, per-source failures are logged and skipped.
pub fn collect(source: Option<SourceArg>) -> Result<Vec<UnifiedMessage>> {
    match source {
        Some(SourceArg::Claude) => collect_claude(),
        Some(SourceArg::Codex) => collect_codex(),
        Some(SourceArg::Cursor) => collect_cursor(),
        Some(SourceArg::OpenCode) => collect_opencode(),
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
            match collect_opencode() {
                Ok(mut v) => out.append(&mut v),
                Err(e) => tracing::warn!("opencode source failed: {e}"),
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

fn collect_opencode() -> Result<Vec<UnifiedMessage>> {
    let root =
        sources::opencode::default_root().context("could not resolve OpenCode data directory")?;
    sources::opencode::scan(&root)
}
