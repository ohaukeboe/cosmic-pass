//! Command-line interface. Defined here so tests can check it against the arguments the app
//! spawns for its own clipboard helper.

use clap::{Parser, Subcommand};

/// Quick-access popup for Proton Pass on COSMIC.
#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Cli {
    /// Start in the background without showing the window.
    #[arg(long)]
    pub background: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The helper is spawned by the app itself, so its argv must parse with this very parser.
    /// A mismatch means every copy fails at runtime while the unit tests (which drive a fake
    /// helper) stay green.
    #[test]
    fn spawned_helper_argv_parses() {
        for secret in [true, false] {
            let args = crate::clipboard::helper_args(secret, Duration::from_secs(90));
            let argv: Vec<String> = std::iter::once("cosmic-pass".to_owned())
                .chain(std::iter::once("clipboard-serve".to_owned()))
                .chain(args)
                .collect();
            let cli = Cli::try_parse_from(&argv)
                .unwrap_or_else(|e| panic!("secret={secret}: helper rejects its own argv: {e}"));
            match cli.command {
                Some(Command::ClipboardServe {
                    timeout,
                    secret: got,
                }) => {
                    assert_eq!(timeout, 90);
                    assert_eq!(got, secret);
                }
                other => panic!("expected clipboard-serve, got {other:?}"),
            }
        }
    }

    #[test]
    fn timeout_is_required_by_the_parser() {
        // Documents why helper_args always passes it, whatever the copy's secrecy.
        assert!(Cli::try_parse_from(["cosmic-pass", "clipboard-serve"]).is_err());
    }

    #[test]
    fn plain_invocation_toggles() {
        let cli = Cli::try_parse_from(["cosmic-pass"]).unwrap();
        assert!(cli.command.is_none() && !cli.background);
    }
}
