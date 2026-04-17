use crate::{claude_hook, config, manifest};
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
    uninstall_scheduler();

    match claude_hook::remove() {
        Ok(true) => println!("  claude    : removed managed Stop hook"),
        Ok(false) => println!("  claude    : no managed hook found"),
        Err(e) => eprintln!("  claude    : failed to clean up hook: {e}"),
    }

    if let Some(m) = manifest_opt {
        for f in &m.files {
            manifest::remove_file_best_effort(f);
        }
    }

    if let Ok(dir) = manifest::data_dir() {
        manifest::remove_dir_all_best_effort(&dir);
        println!("  data      : removed {}", dir.display());
    }
    if let Ok(dir) = config::config_dir() {
        manifest::remove_dir_all_best_effort(&dir);
        println!("  config    : removed {}", dir.display());
    }

    println!("done. The binary at $(which tokusage) is left in place; remove it manually.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_scheduler() {
    use crate::platform::macos;
    if let Ok(plist) = macos::plist_path() {
        macos::launchctl_unload(&plist).ok();
        manifest::remove_file_best_effort(&plist);
        println!("  launchd   : removed {}", plist.display());
    }
}

#[cfg(target_os = "linux")]
fn uninstall_scheduler() {
    use crate::platform::linux;
    linux::systemctl_disable_and_stop().ok();
    if let Ok(svc) = linux::service_path() {
        manifest::remove_file_best_effort(&svc);
    }
    if let Ok(timer) = linux::timer_path() {
        manifest::remove_file_best_effort(&timer);
    }
    println!("  systemd   : removed user units and disabled timer");
}

#[cfg(target_os = "windows")]
fn uninstall_scheduler() {
    use crate::platform::windows;
    windows::delete_task().ok();
    println!(
        "  scheduler : deleted Windows Task Scheduler task '{}'",
        windows::TASK_NAME
    );
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn uninstall_scheduler() {
    eprintln!("  scheduler : (this platform has no installer)");
}
