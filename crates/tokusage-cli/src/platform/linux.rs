//! Linux systemd user-unit integration.
//!
//! `install()` writes two files under `~/.config/systemd/user/`:
//! - `tokusage.service` — one-shot unit that runs `tokusage submit`
//! - `tokusage.timer`   — activates the service every 2h, with
//!   `Persistent=true` so misses (sleep, power-off) are caught up on boot
//!
//! Then `systemctl --user daemon-reload && systemctl --user enable --now tokusage.timer`.
//! `uninstall()` reverses both.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub const SERVICE: &str = "tokusage.service";
pub const TIMER: &str = "tokusage.timer";
/// Every 2 hours.
pub const INTERVAL: &str = "2h";

fn units_dir() -> Result<PathBuf> {
    let dirs = directories::BaseDirs::new()
        .context("could not determine user home directory")?;
    Ok(dirs.config_dir().join("systemd/user"))
}

pub fn service_path() -> Result<PathBuf> {
    Ok(units_dir()?.join(SERVICE))
}

pub fn timer_path() -> Result<PathBuf> {
    Ok(units_dir()?.join(TIMER))
}

/// Returns the (service_path, timer_path) of the files written.
pub fn write_units(
    binary_path: &std::path::Path,
    log_path: &std::path::Path,
) -> Result<(PathBuf, PathBuf)> {
    let dir = units_dir()?;
    fs::create_dir_all(&dir)?;

    let service = format!(
        r#"[Unit]
Description=tokusage - submit AI coding tool token usage
Documentation=https://github.com/Enmming/tokusage

[Service]
Type=oneshot
ExecStart={bin} submit
StandardOutput=append:{log}
StandardError=append:{log}
"#,
        bin = binary_path.display(),
        log = log_path.display(),
    );

    let timer = format!(
        r#"[Unit]
Description=Run tokusage every {interval}

[Timer]
OnBootSec=2min
OnUnitActiveSec={interval}
Persistent=true
Unit={svc}

[Install]
WantedBy=timers.target
"#,
        interval = INTERVAL,
        svc = SERVICE,
    );

    let sp = service_path()?;
    let tp = timer_path()?;
    fs::write(&sp, service)?;
    fs::write(&tp, timer)?;
    Ok((sp, tp))
}

pub fn systemctl_enable_and_start() -> Result<()> {
    run(&["daemon-reload"])?;
    run(&["enable", "--now", TIMER])?;
    Ok(())
}

pub fn systemctl_disable_and_stop() -> Result<()> {
    // Best effort — ignore errors so uninstall still proceeds on partially
    // broken installs.
    let _ = run(&["disable", "--now", TIMER]);
    let _ = run(&["daemon-reload"]);
    Ok(())
}

fn run(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .context("failed to invoke systemctl")?;
    if !status.success() {
        anyhow::bail!("systemctl --user {:?} exited with {}", args, status);
    }
    Ok(())
}
