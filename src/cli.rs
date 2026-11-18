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
    /// Run without a window for another front end: JSON intents on stdin, JSON snapshots on
    /// stdout (specs/004-serve-mode/contracts/serve-protocol.md).
    Serve {
        /// Seconds a copied secret stays on the clipboard.
        #[arg(long, value_parser = clap::value_parser!(u32).range(10..=600))]
        clipboard_clear_secs: u32,
    },
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

    /// The flag's range must be the one the preferences enforce, or `serve` would accept a
    /// timeout the popup clamps away.
    #[test]
    fn serve_takes_the_preference_range() {
        let parse = |secs: &str| {
            Cli::try_parse_from(["cosmic-pass", "serve", "--clipboard-clear-secs", secs])
                .map(|cli| cli.command)
        };
        let (low, high) = (
            *crate::config::CLIPBOARD_CLEAR_RANGE.start(),
            *crate::config::CLIPBOARD_CLEAR_RANGE.end(),
        );
        for ok in [low, high] {
            assert_eq!(
                parse(&ok.to_string()).unwrap(),
                Some(Command::Serve {
                    clipboard_clear_secs: ok
                })
            );
        }
        for bad in [low - 1, high + 1] {
            assert!(parse(&bad.to_string()).is_err(), "{bad} accepted");
        }
        assert!(Cli::try_parse_from(["cosmic-pass", "serve"]).is_err());
    }

    #[test]
    fn plain_invocation_toggles() {
        let cli = Cli::try_parse_from(["cosmic-pass"]).unwrap();
        assert!(cli.command.is_none() && !cli.background);
    }
}
