//! Errors from `pass-cli` and classification of its stderr.

use std::io;

const MAX_MESSAGE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PassError {
    #[error("pass-cli is not installed")]
    CliMissing,
    #[error("not signed in to Proton Pass")]
    SignedOut,
    #[error("the Proton Pass session is locked")]
    Locked,
    #[error("network error while talking to Proton Pass")]
    Network,
    #[error("pass-cli did not respond in time")]
    Timeout,
    #[error("item or vault not found")]
    NotFound,
    #[error("this item has no such field")]
    FieldMissing,
    #[error("cancelled")]
    Cancelled,
    #[error("unexpected output from pass-cli ({command})")]
    Protocol { command: &'static str },
    #[error("pass-cli failed: {message}")]
    Cli { message: String },
}

/// How a `pass-cli` process ended, for classification.
#[derive(Debug, Default)]
pub struct Failure<'a> {
    pub spawn_error: Option<io::ErrorKind>,
    pub timed_out: bool,
    pub stderr: &'a str,
}

/// Maps a failed `pass-cli` run to a [`PassError`] using the ordered table in
/// `contracts/pass-cli.md`. Only stderr is inspected; stdout may hold secrets.
pub fn classify(failure: &Failure<'_>) -> PassError {
    match failure.spawn_error {
        Some(io::ErrorKind::NotFound) => return PassError::CliMissing,
        Some(kind) => {
            return PassError::Cli {
                message: format!("could not start pass-cli: {kind}"),
            };
        }
        None => {}
    }
    if failure.timed_out {
        return PassError::Timeout;
    }

    let text = failure.stderr.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
    if has(&["requires an authenticated client", "there is no session"]) {
        PassError::SignedOut
    } else if has(&["field does not exist"]) {
        PassError::FieldMissing
    } else if has(&["locked"]) {
        PassError::Locked
    } else if has(&["could not find", "error finding item", "idformat"]) {
        PassError::NotFound
    } else if has(&["connection", "timed out", "dns", "network"]) {
        PassError::Network
    } else {
        PassError::Cli {
            message: summary_line(failure.stderr),
        }
    }
}

/// The first `Error:` line without its prefix, else the last non-empty line.
fn summary_line(stderr: &str) -> String {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    let line = lines()
        .find_map(|l| l.strip_prefix("Error:").map(str::trim))
        .or_else(|| lines().next_back())
        .unwrap_or("pass-cli exited with an error");
    line.chars().take(MAX_MESSAGE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stderr(text: &str) -> PassError {
        classify(&Failure {
            stderr: text,
            ..Failure::default()
        })
    }

    #[test]
    fn spawn_not_found_is_cli_missing() {
        let f = Failure {
            spawn_error: Some(io::ErrorKind::NotFound),
            ..Failure::default()
        };
        assert_eq!(classify(&f), PassError::CliMissing);
    }

    #[test]
    fn other_spawn_errors_are_cli_errors() {
        let f = Failure {
            spawn_error: Some(io::ErrorKind::PermissionDenied),
            ..Failure::default()
        };
        assert!(matches!(classify(&f), PassError::Cli { .. }));
    }

    #[test]
    fn timeout_wins_over_stderr() {
        let f = Failure {
            timed_out: true,
            stderr: "Error: connection reset",
            ..Failure::default()
        };
        assert_eq!(classify(&f), PassError::Timeout);
    }

    #[test]
    fn signed_out_with_leading_tracing_line() {
        let text = "\x1b[2m2026-09-17T12:11:33Z\x1b[0m \x1b[31mERROR\x1b[0m main.rs:332: \
                    Command is not logout there is no session\n\
                    Error: This operation requires an authenticated client\n";
        assert_eq!(stderr(text), PassError::SignedOut);
    }

    #[test]
    fn field_missing() {
        assert_eq!(
            stderr("Error: Field does not exist: username\n"),
            PassError::FieldMissing
        );
    }

    #[test]
    fn locked() {
        assert_eq!(stderr("Error: Session is locked\n"), PassError::Locked);
    }

    #[test]
    fn not_found_chains() {
        let missing_item = "Error: Error retrieving item\n\nCaused by:\n    \
                            0: Error finding item by name\n    1: Error finding vault by name\n    \
                            2: Could not find vault abc\n";
        assert_eq!(stderr(missing_item), PassError::NotFound);
        let bad_id = "Error: Error listing items\n\nCaused by:\n    0: Error fetching items\n    \
                      1: Could not perform operation. Reason: IdFormat\n";
        assert_eq!(stderr(bad_id), PassError::NotFound);
    }

    #[test]
    fn network_errors() {
        for text in [
            "Error: request failed\n\nCaused by:\n    0: connection error: reset\n",
            "Error: Authentication timed out after 30s\n",
            "Error: dns error: failed to lookup address\n",
            "Error: Network unreachable\n",
        ] {
            assert_eq!(stderr(text), PassError::Network, "{text}");
        }
    }

    #[test]
    fn other_errors_keep_first_error_line_truncated() {
        let long = format!(
            "noise\nError: {}\nCaused by:\n    0: detail\n",
            "x".repeat(300)
        );
        match stderr(&long) {
            PassError::Cli { message } => {
                assert!(message.starts_with("xxx"));
                assert_eq!(message.chars().count(), MAX_MESSAGE_CHARS);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn other_errors_without_error_line_use_last_line() {
        assert_eq!(
            stderr("something odd\nreally odd\n"),
            PassError::Cli {
                message: "really odd".into()
            }
        );
    }

    #[test]
    fn empty_stderr_is_generic_cli_error() {
        assert!(matches!(stderr(""), PassError::Cli { message } if !message.is_empty()));
    }
}
