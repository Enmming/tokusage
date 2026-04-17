use crate::config::{self, Config};
use anyhow::{Context, Result};
use std::io::{self, Write};

pub fn run(api_url: Option<String>, token: Option<String>) -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();

    let api_url = match api_url {
        Some(u) => u,
        None => prompt("Company API base URL (e.g. https://tokusage.yourcorp.com): ")?,
    };
    let token = match token {
        Some(t) => t,
        None => prompt("Company API token: ")?,
    };

    cfg.api_url = Some(api_url.trim().to_string());
    cfg.api_token = Some(token.trim().to_string());
    config::save(&cfg)?;

    println!(
        "Saved credentials to {}",
        config::config_path()?.display()
    );
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin()
        .read_line(&mut buf)
        .context("reading from stdin")?;
    Ok(buf.trim().to_string())
}

/// Return a validated (api_url, api_token) pair from config, or an actionable
/// error if either is missing.
pub fn require_credentials() -> Result<(String, String)> {
    let cfg = config::load()?;
    let url = cfg.api_url.context(
        "no api_url configured. Run 'tokusage login --api-url <URL> --token <TOKEN>' first.",
    )?;
    let token = cfg.api_token.context(
        "no api_token configured. Run 'tokusage login --api-url <URL> --token <TOKEN>' first.",
    )?;
    Ok((url, token))
}
