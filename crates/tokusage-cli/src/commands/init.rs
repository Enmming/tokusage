use crate::{claude_hook, manifest};
use anyhow::{Context, Result};
use std::io::{self, Write};

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn run(_yes: bool) -> Result<()> {
    anyhow::bail!("init: this platform is not supported")
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn run(yes: bool) -> Result<()> {
    let binary = std::env::current_exe().context("could not determine tokusage binary path")?;
    let log = manifest::log_path()?;

    println!("tokusage init");
    println!("  binary    : {}", binary.display());
    println!("  log       : {}", log.display());

    let mut files_created = Vec::new();
    install_scheduler(&binary, &log, &mut files_created)?;

    // Claude hook: opt-in, cross-platform.
    let claude_hook_installed = if yes {
        false
    } else {
        match prompt_yes_no(
            "Inject Claude Code Stop hook so tokusage runs after every Claude response? [y/N]: ",
        )? {
            true => {
                claude_hook::inject(&binary)?;
                println!(
                    "  claude    : injected Stop hook into {}",
                    claude_hook::settings_path()?.display()
                );
                true
            }
            false => {
                println!("  claude    : skipped (no hook)");
                false
            }
        }
    };

    manifest::save(&manifest::InstallManifest {
        version: env!("CARGO_PKG_VERSION").to_string(),
        installed_at: chrono::Utc::now(),
        binary_path: binary,
        files: files_created,
        claude_hook_installed,
    })?;

    println!("done.");
    println!("  run 'tokusage login' next, then 'tokusage submit' to send your first payload.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_scheduler(
    binary: &std::path::Path,
    log: &std::path::Path,
    files: &mut Vec<std::path::PathBuf>,
) -> Result<()> {
    use crate::platform::macos;
    let plist = macos::write_plist(binary, log)?;
    macos::launchctl_load(&plist)?;
    println!("  launchd   : {} (loaded)", plist.display());
    files.push(plist);
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_scheduler(
    binary: &std::path::Path,
    log: &std::path::Path,
    files: &mut Vec<std::path::PathBuf>,
) -> Result<()> {
    use crate::platform::linux;
    let (svc, timer) = linux::write_units(binary, log)?;
    linux::systemctl_enable_and_start()?;
    println!("  systemd   : {}", svc.display());
    println!("              {} (enabled, active)", timer.display());
    files.push(svc);
    files.push(timer);
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_scheduler(
    binary: &std::path::Path,
    _log: &std::path::Path,
    _files: &mut Vec<std::path::PathBuf>,
) -> Result<()> {
    use crate::platform::windows;
    windows::create_task(binary)?;
    println!(
        "  scheduler : Windows Task Scheduler task '{}' created",
        windows::TASK_NAME
    );
    Ok(())
}

fn prompt_yes_no(label: &str) -> Result<bool> {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let ans = buf.trim().to_lowercase();
    Ok(matches!(ans.as_str(), "y" | "yes"))
}
