//! Windows Task Scheduler integration via `schtasks.exe`.
//!
//! Creates a user-level task that runs `tokusage submit` every 2 hours.
//! We shell out to the built-in `schtasks` rather than using the COM Task
//! Scheduler API because it avoids pulling in the `windows` crate and any
//! platform-specific build steps — every Windows box has schtasks.

use anyhow::{Context, Result};
use std::process::Command;

pub const TASK_NAME: &str = "Tokusage";
/// Every 2 hours, in Task Scheduler's XML-friendly notation.
pub const INTERVAL_HOURS: u32 = 2;

pub fn create_task(binary_path: &std::path::Path) -> Result<()> {
    // /F = force overwrite; /RL LIMITED = user-level, no elevation.
    // /SC HOURLY /MO 2 = every 2 hours.
    let status = Command::new("schtasks.exe")
        .args([
            "/Create",
            "/F",
            "/SC",
            "HOURLY",
            "/MO",
            &INTERVAL_HOURS.to_string(),
            "/RL",
            "LIMITED",
            "/TN",
            TASK_NAME,
            "/TR",
            &format!("\"{}\" submit", binary_path.display()),
        ])
        .status()
        .context("failed to invoke schtasks.exe")?;

    if !status.success() {
        anyhow::bail!("schtasks /Create exited with {}", status);
    }
    Ok(())
}

pub fn delete_task() -> Result<()> {
    // Best effort; swallow errors so a missing task doesn't block uninstall.
    let _ = Command::new("schtasks.exe")
        .args(["/Delete", "/F", "/TN", TASK_NAME])
        .status();
    Ok(())
}
