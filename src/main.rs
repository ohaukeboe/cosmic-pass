//! `cosmic-pass` binary entry point.

use clap::{Parser, Subcommand};
use cosmic_pass::app::{self, Flags, RemoteAction};

/// Quick-access popup for Proton Pass on COSMIC.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Start in the background without showing the window.
    #[arg(long)]
    background: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Show the window.
    Show,
    /// Hide the window.
    Hide,
    /// Refresh items in the background.
    Refresh,
    /// Internal: serve a value from stdin on the clipboard.
    #[command(hide = true)]
    ClipboardServe {
        #[arg(long)]
        timeout: u64,
        #[arg(long)]
        secret: bool,
    },
}

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let action = match cli.command {
        Some(Command::ClipboardServe { timeout, secret }) => {
            return cosmic_pass::clipboard::serve::main(timeout, secret);
        }
        Some(Command::Show) => Some(RemoteAction::Show),
        Some(Command::Hide) => Some(RemoteAction::Hide),
        Some(Command::Refresh) => Some(RemoteAction::Refresh),
        None if cli.background => Some(RemoteAction::Background),
        None => None,
    };
    match app::run(Flags { action }) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
