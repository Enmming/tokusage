use crate::{config, manifest, queue};
use anyhow::Result;

pub fn run() -> Result<()> {
    println!("tokusage {}", env!("CARGO_PKG_VERSION"));
    println!("  host id     : {}", manifest::host_id());

    // Config
    match config::load() {
        Ok(cfg) => {
            let url = cfg.api_url.as_deref().unwrap_or("(not set)");
            let token = cfg
                .api_token
                .as_deref()
                .map(mask)
                .unwrap_or_else(|| "(not set)".to_string());
            println!("  api url     : {}", url);
            println!("  api token   : {}", token);
            println!(
                "  config file : {}",
                config::config_path()?.display()
            );
        }
        Err(e) => println!("  config      : error loading: {}", e),
    }

    // Manifest
    match manifest::load() {
        Ok(Some(m)) => {
            println!("  installed   : {} (v{})", m.installed_at, m.version);
            println!("  binary      : {}", m.binary_path.display());
            println!("  claude hook : {}", m.claude_hook_installed);
        }
        Ok(None) => println!("  installed   : no (run 'tokusage init')"),
        Err(e) => println!("  manifest    : error: {}", e),
    }

    // Queue
    match queue::list() {
        Ok(q) => {
            println!("  queued      : {} payload(s)", q.len());
            for p in q.iter().take(5) {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    println!("                  {}", name);
                }
            }
        }
        Err(e) => println!("  queue       : error: {}", e),
    }

    // Last log activity gives a rough "last submit" timestamp.
    if let Ok(log) = manifest::log_path() {
        match std::fs::metadata(&log) {
            Ok(m) => {
                if let Ok(mtime) = m.modified() {
                    let dt: chrono::DateTime<chrono::Utc> = mtime.into();
                    println!("  last run    : {}", dt);
                }
            }
            Err(_) => println!("  last run    : never"),
        }
    }

    Ok(())
}

fn mask(s: &str) -> String {
    let len = s.len();
    if len <= 8 {
        "*".repeat(len)
    } else {
        format!("{}…{}", &s[..4], &s[len - 4..])
    }
}
