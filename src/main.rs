//! `cosmic-pass` binary entry point.

use clap::Parser;
use cosmic_pass::app::{self, Flags, RemoteAction};
use cosmic_pass::cli::{Cli, Command};

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
