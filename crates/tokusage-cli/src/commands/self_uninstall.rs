use crate::{claude_hook, config, manifest, platform};
use anyhow::Result;
use std::io::{self, Write};

pub fn run(yes: bool) -> Result<()> {
    if !yes {
        print!("Remove tokusage scheduler, Claude hook, config, and data directory? [y/N]: ");
        io::stdout().flush().ok();
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        if !matches!(buf.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("aborted.");
            return Ok(());
        }
    }

    let manifest_opt = manifest::load().ok().flatten();

    // launchd — unload by label-path even if manifest is missing, so a
    // half-installed state can still be cleaned up.
    #[cfg(target_os = "macos")]
    {
        if let Ok(plist) = platform::macos::plist_path() {
            platform::macos::launchctl_unload(&plist).ok();
            manifest::remove_file_best_effort(&plist);
            println!("  launchd : removed {}", plist.display());
        }
    }

    // Claude hook
    match claude_hook::remove() {
        Ok(true) => println!("  claude  : removed managed Stop hook"),
        Ok(false) => println!("  claude  : no managed hook found"),
        Err(e) => eprintln!("  claude  : failed to clean up hook: {e}"),
    }

    // Manifest-tracked files (for forward compatibility if we track more later)
    if let Some(m) = manifest_opt {
        for f in &m.files {
            manifest::remove_file_best_effort(f);
        }
    }

    // Data directory, config directory
    if let Ok(dir) = manifest::data_dir() {
        manifest::remove_dir_all_best_effort(&dir);
        println!("  data    : removed {}", dir.display());
    }
    if let Ok(dir) = config::config_dir() {
        manifest::remove_dir_all_best_effort(&dir);
        println!("  config  : removed {}", dir.display());
    }

    println!("done. The binary at $(which tokusage) is left in place; remove it manually.");
    Ok(())
}
