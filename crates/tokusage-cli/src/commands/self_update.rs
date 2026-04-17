//! Lightweight self-updater. Queries GitHub Releases for the latest tag, and
//! if it differs from the compiled-in CARGO_PKG_VERSION, delegates to the
//! install.sh script that ships with each release. We re-exec install.sh
//! rather than reimplementing the download flow so there's exactly one
//! code path that has to keep working.

use anyhow::{Context, Result};
use std::process::Command;

const REPO: &str = "gd/tokusage";

pub fn run() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("current version : {}", current);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let latest = rt.block_on(fetch_latest_tag())?;
    println!("latest release  : {}", latest);

    let latest_num = latest.trim_start_matches('v');
    if latest_num == current {
        println!("already up to date.");
        return Ok(());
    }

    println!("updating...");
    let url = format!(
        "https://github.com/{}/releases/download/{}/install.sh",
        REPO, latest
    );
    // Fetch + exec install.sh in a subshell. This mirrors what a human would
    // do: curl ... | bash. We keep it explicit so the user sees the command.
    let status = Command::new("bash")
        .arg("-c")
        .arg(format!("curl -fsSL {} | bash", url))
        .status()
        .context("running install.sh")?;
    if !status.success() {
        anyhow::bail!("install.sh exited with {}", status);
    }
    Ok(())
}

async fn fetch_latest_tag() -> Result<String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let client = reqwest::Client::builder()
        .user_agent(format!("tokusage/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .context("GET latest release")?
        .error_for_status()?;
    let body: serde_json::Value = resp.json().await?;
    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .context("no tag_name in GitHub response")?;
    Ok(tag.to_string())
}
