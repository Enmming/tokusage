use crate::{claude_hook, manifest};
use anyhow::{Context, Result};
use std::io::{self, Write};

#[cfg(not(target_os = "macos"))]
pub fn run(_yes: bool) -> Result<()> {
    anyhow::bail!("init: only macOS is supported in this milestone")
}

#[cfg(target_os = "macos")]
pub fn run(yes: bool) -> Result<()> {
    use crate::platform;

    let binary = std::env::current_exe().context("could not determine tokusage binary path")?;
    let log = manifest::log_path()?;

    println!("tokusage init");
    println!("  binary  : {}", binary.display());
    println!("  log     : {}", log.display());

    let mut files_created = Vec::new();

    let plist = platform::macos::write_plist(&binary, &log)?;
    platform::macos::launchctl_load(&plist)?;
    println!("  launchd : {} (loaded)", plist.display());
    files_created.push(plist);

    // Claude hook is opt-in.
    let claude_hook_installed = if yes {
        false
    } else {
        match prompt_yes_no("Inject Claude Code Stop hook so tokusage runs after every Claude response? [y/N]: ")? {
            true => {
                claude_hook::inject(&binary)?;
                println!(
                    "  claude  : injected Stop hook into {}",
                    claude_hook::settings_path()?.display()
                );
                true
            }
            false => {
                println!("  claude  : skipped (no hook)");
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
fn prompt_yes_no(label: &str) -> Result<bool> {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let ans = buf.trim().to_lowercase();
    Ok(matches!(ans.as_str(), "y" | "yes"))
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn _unused_imports_workaround() {
    // Keeps io::Write/Context imported for the macOS build path without
    // triggering unused_imports on Linux.
    let _ = (io::stdout(), || -> Result<()> { Ok(()) });
}
