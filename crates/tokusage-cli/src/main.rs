use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

mod claude_hook;
mod commands;
mod config;
mod manifest;
mod platform;
mod queue;

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
enum SourceArg {
    Claude,
    Codex,
    Cursor,
}

#[derive(Parser)]
#[command(
    name = "tokusage",
    version,
    about = "Track AI coding tool token usage across Claude Code, Codex, and Cursor"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install launchd scheduler, write manifest, optionally inject Claude hook
    Init {
        /// Non-interactive: skip prompts, use defaults (no Claude hook)
        #[arg(long)]
        yes: bool,
    },
    /// Configure company API endpoint and token
    Login {
        /// Company API base URL (e.g. https://tokusage.yourcorp.com)
        #[arg(long)]
        api_url: Option<String>,
        /// Company API token
        #[arg(long)]
        token: Option<String>,
    },
    /// Scan sources and submit aggregated usage to the configured API
    Submit {
        /// Print the payload without actually submitting
        #[arg(long)]
        dry_run: bool,
        /// Only run a single source (claude|codex|cursor)
        #[arg(long)]
        source: Option<SourceArg>,
    },
    /// Show config, last submit, and queue state
    Status,
    /// Check GitHub Releases and replace self with the latest version
    SelfUpdate,
    /// Remove launchd, Claude hook, manifest, and all tokusage files
    SelfUninstall {
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("TOKUSAGE_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Init { yes } => commands::init::run(yes),
        Command::Login { api_url, token } => commands::login::run(api_url, token),
        Command::Submit { dry_run, source } => commands::submit::run(dry_run, source),
        Command::Status => commands::status::run(),
        Command::SelfUpdate => commands::self_update::run(),
        Command::SelfUninstall { yes } => commands::self_uninstall::run(yes),
    }
}
