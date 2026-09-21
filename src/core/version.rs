//! What `pass-cli` versions this app is known to drive, and what to say about the rest.
//!
//! `pass-cli` publishes no stability policy and changes its command surface in patch
//! releases: 2.1.4 made the section name part of a field's address, 2.2.2 renamed
//! `session lock` and reused the old name for something else, 2.2.4 dropped a command
//! outright. A version number therefore carries no compatibility promise, and the only
//! honest thing to do is name the versions that were actually exercised.
//!
//! The packaged install wraps the binary with `--suffix PATH`, so a `pass-cli` the user
//! installed themselves still wins. No packaging choice can pin what runs; only this check
//! sees it.

use crate::model::CliVersion;

/// The oldest `pass-cli` this app is known to work with. Below it, `item view --field` cannot
/// address a field inside a section, so those fields fail to copy.
pub const TESTED_MIN: CliVersion = CliVersion::new(2, 3, 0);

/// The warning line for a `pass-cli` outside the tested range, or `None` when it is inside.
///
/// A warning, never a refusal: an older `pass-cli` still copies most fields, and the user is
/// better served by a working popup that says what is wrong than by one that will not open.
/// Newer-than-tested is deliberately silent — upstream ships about weekly, so warning on it
/// would leave the line permanently on screen and teach the user to ignore it.
pub fn warning(found: CliVersion) -> Option<String> {
    (found < TESTED_MIN).then(|| {
        format!(
            "pass-cli {found} is older than the tested {TESTED_MIN}; some fields may not copy. \
             Upgrade pass-cli."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_at_or_above_the_floor_are_quiet() {
        assert_eq!(warning(TESTED_MIN), None);
        assert_eq!(warning(CliVersion::new(2, 3, 3)), None);
        // Newer than tested stays quiet on purpose, however far ahead.
        assert_eq!(warning(CliVersion::new(3, 0, 0)), None);
    }

    #[test]
    fn older_versions_name_both_versions() {
        // What nixos-26.05 ships.
        let text = warning(CliVersion::new(2, 0, 2)).expect("a warning");
        assert!(text.contains("2.0.2"), "{text}");
        assert!(text.contains("2.3.0"), "{text}");
        assert!(text.contains("Upgrade pass-cli"), "{text}");
    }
}
