//! Asserts the real `pass-cli` still offers the interface the app drives.
//!
//! No Proton account, no network, no D-Bus: every probe runs against a throwaway home, so this
//! suite belongs in the default gate. `tests/pass_cli_integration.rs` is the sibling suite that
//! drives `tests/fixtures/fake-pass-cli`; it proves the app's own logic, this one proves the
//! assumptions that logic is built on.
//!
//! Each probe names the clause of `specs/001-quick-access-launcher/contracts/pass-cli.md` it
//! defends, so a failure says what broke rather than only where.

mod support;

use cosmic_pass::pass::error::PassError;
use cosmic_pass::pass::runner::CommandRunner;
use support::real_cli::{IsolatedEnv, PROBE_TIMEOUT, RealCli, args, or_skip};
use tokio_util::sync::CancellationToken;

/// Fails the probe unless `pass-cli` rejected the call for lack of a session.
///
/// Signed-out is the *success* condition for a contract probe. It means two things at once:
/// the argv parsed (a rejected argument would surface as `Cli`, clap's usage error), and the
/// isolation held (a real session would have answered).
fn assert_signed_out(clause: &str, result: Result<impl std::fmt::Debug, PassError>) {
    match result {
        Err(PassError::SignedOut) => {}
        Err(PassError::Cli { message }) => panic!(
            "{clause}: pass-cli rejected the call itself, so the app's argv no longer parses: {message}"
        ),
        Err(other) => panic!("{clause}: expected SignedOut, got {other:?}"),
        Ok(_) => panic!(
            "{clause}: the probe reached an authenticated session, so its isolation failed \
             and it was talking to the developer's real account"
        ),
    }
}

mod isolation {
    use super::*;

    /// Guarantee 1 of the harness contract. Every other probe in this file is meaningless
    /// without it: if isolation leaks, the suite is silently driving a real vault.
    #[tokio::test]
    async fn probe_is_unauthenticated() {
        let Some(cli) = or_skip() else { return };
        let home = IsolatedEnv::new();
        let result = home
            .runner(cli)
            .run(
                args(&["info", "--output", "json"]),
                PROBE_TIMEOUT,
                CancellationToken::new(),
            )
            .await;
        assert_signed_out("isolation", result);
    }

    /// Guarantee 2. `pass-cli` keeps its store under `XDG_DATA_HOME`, so a probe that honours
    /// the override writes only there. Measured on 2.3.3: one file,
    /// `share/proton-pass-cli/.session/pass-cli.db`.
    #[tokio::test]
    async fn probe_writes_only_inside_its_own_home() {
        let Some(cli) = or_skip() else { return };
        let home = IsolatedEnv::new();
        let _ = home
            .runner(cli)
            .run(
                args(&["info", "--output", "json"]),
                PROBE_TIMEOUT,
                CancellationToken::new(),
            )
            .await;

        let written = home.files_written();
        assert!(
            !written.is_empty(),
            "pass-cli wrote nothing at all, so this probe proves nothing about where it writes"
        );
        let stray: Vec<_> = written
            .iter()
            .filter(|p| !p.starts_with("share/proton-pass-cli"))
            .collect();
        assert!(
            stray.is_empty(),
            "pass-cli wrote outside its own store inside the isolated home: {stray:?}"
        );
    }
}

mod version {
    use super::*;
    use cosmic_pass::core::version::TESTED_MIN;

    /// Resolution already fails the run below the floor; this asserts that rule held and
    /// records what was actually exercised, which is the signal for raising `TESTED_MIN`.
    #[test]
    fn reported_version_is_at_or_above_tested_min() {
        let Some(cli) = or_skip() else { return };
        assert!(
            cli.version >= TESTED_MIN,
            "pass-cli {} is below the tested floor {TESTED_MIN}",
            cli.version
        );
        eprintln!(
            "pass-cli under test: {} ({}), tested floor {TESTED_MIN}",
            cli.version,
            cli.path.display()
        );
    }
}

/// Command and flag surface.
///
/// `--help` needs no session and exits 0, so it separates "the flag was removed" from "the flag
/// exists but you are signed out" -- two outcomes that a failure-text probe would conflate.
mod surface {
    use super::*;

    fn help(cli: &RealCli, subcommand: &[&str]) -> support::real_cli::RawOutput {
        let home = IsolatedEnv::new();
        let mut argv = subcommand.to_vec();
        argv.push("--help");
        home.raw(cli, &argv)
    }

    #[test]
    fn top_level_commands_exist() {
        let Some(cli) = or_skip() else { return };
        help(cli, &[]).assert_stdout_has(
            "top-level commands",
            &["login", "logout", "info", "vault", "item"],
        );
    }

    #[test]
    fn info_accepts_output_json() {
        let Some(cli) = or_skip() else { return };
        help(cli, &["info"]).assert_stdout_has("info --output json", &["--output", "json"]);
    }

    #[test]
    fn vault_list_accepts_output_json() {
        let Some(cli) = or_skip() else { return };
        help(cli, &["vault", "list"])
            .assert_stdout_has("vault list --output json", &["--output", "json"]);
    }

    #[test]
    fn item_list_accepts_share_id_output_and_show_secrets() {
        let Some(cli) = or_skip() else { return };
        help(cli, &["item", "list"]).assert_stdout_has(
            "item list --share-id=<S> --output json --show-secrets",
            &["--share-id", "--output", "--show-secrets"],
        );
    }

    #[test]
    fn item_view_accepts_share_id_item_id_and_field() {
        let Some(cli) = or_skip() else { return };
        help(cli, &["item", "view"]).assert_stdout_has(
            "item view --share-id=<S> --item-id=<I> --field=<F>",
            &["--share-id", "--item-id", "--field"],
        );
    }

    #[test]
    fn item_totp_accepts_share_id_item_id_and_output() {
        let Some(cli) = or_skip() else { return };
        help(cli, &["item", "totp"]).assert_stdout_has(
            "item totp --share-id=<S> --item-id=<I> --output json",
            &["--share-id", "--item-id", "--output"],
        );
    }
}

/// The argv the app actually builds, not just the tokens help advertises.
///
/// Sourced from `cosmic_pass::pass::backend::argv`, never retyped: a copy here would keep
/// passing after the app started sending something else, which is the one failure this suite
/// exists to prevent.
mod argv {
    use super::*;
    use cosmic_pass::model::ItemKey;
    use cosmic_pass::pass::backend::argv as app;

    /// Ids that resolve to nothing. The probe is signed out, so they are never looked up; they
    /// exist only to make the argv complete.
    fn key() -> ItemKey {
        ItemKey::new("contract-test-share", "contract-test-item")
    }

    fn every_command() -> Vec<(&'static str, Vec<String>)> {
        vec![
            ("info", app::info()),
            ("vault list", app::vault_list()),
            ("item list", app::item_list("contract-test-share")),
            ("item view", app::item_view(&key(), "password")),
            ("item totp", app::item_totp(&key())),
        ]
    }

    #[tokio::test]
    async fn every_command_the_app_builds_is_accepted() {
        let Some(cli) = or_skip() else { return };
        let home = IsolatedEnv::new();
        let runner = home.runner(cli);
        for (clause, argv) in every_command() {
            let result = runner
                .run(argv, PROBE_TIMEOUT, CancellationToken::new())
                .await;
            assert_signed_out(clause, result);
        }
    }

    /// `--version` is the one command that answers without a session, so it gets its own probe.
    #[tokio::test]
    async fn the_version_command_the_app_builds_answers() {
        let Some(cli) = or_skip() else { return };
        let home = IsolatedEnv::new();
        let out = home
            .runner(cli)
            .run(app::version(), PROBE_TIMEOUT, CancellationToken::new())
            .await
            .expect("--version needs no session");
        assert_eq!(
            cosmic_pass::pass::parse::parse_version(&out.stdout),
            Ok(cli.version)
        );
    }

    /// Share ids can begin with `-`, so `app::item_list` passes ids as `--flag=VALUE`. That is
    /// a property of `pass-cli`'s argument parser, not a style choice, so it needs its own
    /// test -- and the app's own form is checked against it rather than assumed.
    #[test]
    fn ids_must_use_the_equals_form() {
        let Some(cli) = or_skip() else { return };
        let home = IsolatedEnv::new();

        assert!(
            app::item_list("-leadingdash").contains(&"--share-id=-leadingdash".to_owned()),
            "the app stopped passing the share id as --flag=VALUE: {:?}",
            app::item_list("-leadingdash")
        );

        let spaced = home.raw(
            cli,
            &[
                "item",
                "list",
                "--share-id",
                "-leadingdash",
                "--output",
                "json",
            ],
        );
        assert!(
            spaced.stderr.contains("unexpected argument"),
            "the space-separated form no longer fails, so the reason the app uses \
             --flag=VALUE may have gone away\n--- stderr ---\n{}",
            spaced.stderr
        );

        let equals = home.raw(
            cli,
            &[
                "item",
                "list",
                "--share-id=-leadingdash",
                "--output",
                "json",
            ],
        );
        assert!(
            !equals.stderr.contains("unexpected argument"),
            "--share-id=<value> no longer accepts a leading dash, so ids the app sends are \
             rejected before authentication\n--- stderr ---\n{}",
            equals.stderr
        );
        assert_eq!(
            equals.classified(),
            PassError::SignedOut,
            "the equals form should have got as far as the session check"
        );
    }
}

mod classify {
    use super::*;

    /// The app never sees the raw text; it sees whatever `classify` makes of it. So the
    /// assertion is on the classification, with the text included in the failure message for
    /// whoever has to update the rule.
    #[test]
    fn unauthenticated_stderr_is_signed_out() {
        let Some(cli) = or_skip() else { return };
        let home = IsolatedEnv::new();
        for argv in [
            vec!["info", "--output", "json"],
            vec!["vault", "list", "--output", "json"],
        ] {
            let out = home.raw(cli, &argv);
            assert_eq!(
                out.classified(),
                PassError::SignedOut,
                "{argv:?}: pass-cli's signed-out wording changed, so the app would now \
                 show a raw error instead of the sign-in prompt\n--- stderr ---\n{}",
                out.stderr
            );
        }
    }
}

mod env {
    use super::*;

    /// The app parses stdout and classifies stderr, so only stdout has to be clean.
    ///
    /// Deliberately asserts nothing about stderr: `PASS_LOG_LEVEL=off` does *not* silence
    /// error-level logging on 2.3.3, which is harmless here but would make a "stderr is quiet"
    /// assertion fail for no user-visible reason.
    #[test]
    fn stdout_carries_payload_only() {
        let Some(cli) = or_skip() else { return };
        let home = IsolatedEnv::new();

        let version = home.raw(cli, &["--version"]);
        assert_eq!(version.status, Some(0));
        assert_eq!(
            version.stdout.lines().count(),
            1,
            "the version banner gained extra stdout lines: {:?}",
            version.stdout
        );

        let failed = home.raw(cli, &["info", "--output", "json"]);
        assert!(
            failed.stdout.trim().is_empty(),
            "a failed call wrote to stdout, which the app would try to parse as JSON: {:?}",
            failed.stdout
        );
    }
}

/// FR-013: every probe carries a timeout and kills the process on expiry, so a `pass-cli` that
/// hangs fails the suite instead of wedging it. Asserted against a stand-in that hangs on
/// purpose -- the real binary cannot be made to hang on demand, and a probe whose timeout has
/// never been observed to fire is indistinguishable from one that has none.
mod timeout {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::support::real_cli::RawOutput;

    /// Long enough that the deadline is unambiguously the thing that fired, short enough that
    /// this test costs the suite almost nothing.
    const DEADLINE: Duration = Duration::from_millis(200);
    /// The stand-in outlives its deadline by this much, then would leave a marker behind.
    const OUTLIVES_BY: Duration = Duration::from_secs(2);

    #[test]
    fn a_hung_probe_is_killed_rather_than_waited_on() {
        let home = tempfile::tempdir().expect("temp dir");
        let marker = home.path().join("survived");
        // Sleeps past the deadline, then records that it was still alive. The marker is the
        // assertion that matters: a probe that merely stopped *waiting* for the child would
        // leave it running, and the file would appear.
        let script = format!("sleep {}; : > {}", OUTLIVES_BY.as_secs(), marker.display());

        let started = Instant::now();
        // Matched rather than `expect_err`, which would need `RawOutput: Debug` -- and a
        // `RawOutput` can hold a field value, so it deliberately has no way to be printed.
        let reason =
            match RawOutput::try_capture(Path::new("sh"), &["-c", &script], Vec::new(), DEADLINE) {
                Err(reason) => reason,
                Ok(_) => panic!("a probe that outlives its deadline must fail, not return output"),
            };

        assert!(
            reason.contains("did not finish") && reason.contains("killed"),
            "the failure must say the probe timed out and was killed: {reason}"
        );
        assert!(
            started.elapsed() < OUTLIVES_BY,
            "try_capture waited {:?}, so it waited the child out instead of killing it",
            started.elapsed()
        );

        std::thread::sleep(OUTLIVES_BY + DEADLINE);
        assert!(
            !marker.exists(),
            "the child outlived the probe that spawned it: {} exists",
            marker.display()
        );
    }
}

/// Unit tests for the shape derivation the live suite's fixture check is built on. They need
/// no binary, so they run in the default gate rather than behind the live opt-in.
mod shape {
    use super::support::real_cli::{FixtureShape, ValueKind};

    fn shape(json: &str) -> Vec<(String, ValueKind)> {
        FixtureShape::from_slice(json.as_bytes())
            .expect("valid json")
            .paths
            .into_iter()
            .collect()
    }

    #[test]
    fn collapses_array_indices_into_one_path() {
        assert_eq!(
            shape(r#"{"vaults":[{"share_id":"s"}]}"#),
            vec![
                ("vaults".to_owned(), ValueKind::Array),
                ("vaults[]".to_owned(), ValueKind::Object),
                ("vaults[].share_id".to_owned(), ValueKind::String),
            ]
        );
    }

    #[test]
    fn many_elements_of_one_kind_collapse_to_one_entry() {
        let one = shape(r#"{"v":[{"a":"x"}]}"#);
        let many = shape(r#"{"v":[{"a":"x"},{"a":"y"},{"a":"z"}]}"#);
        assert_eq!(one, many, "element count must not change the shape");
    }

    #[test]
    fn a_field_present_on_only_one_element_still_appears() {
        // Fixtures hold a sample of items, so a field that only some items carry must still
        // register -- otherwise the live check would report it as newly added every run.
        assert!(
            shape(r#"{"v":[{"a":"x"},{"a":"y","b":1}]}"#)
                .contains(&("v[].b".to_owned(), ValueKind::Number))
        );
    }

    #[test]
    fn nested_arrays_keep_one_marker_per_level() {
        assert_eq!(
            shape(r#"{"a":[[1]]}"#),
            vec![
                ("a".to_owned(), ValueKind::Array),
                ("a[]".to_owned(), ValueKind::Array),
                ("a[][]".to_owned(), ValueKind::Number),
            ]
        );
    }

    #[test]
    fn null_is_its_own_kind_so_a_type_change_is_visible() {
        assert_eq!(
            shape(r#"{"note":null}"#),
            vec![("note".to_owned(), ValueKind::Null)]
        );
        let (added, removed) = FixtureShape::from_slice(br#"{"note":"x"}"#)
            .unwrap()
            .diff(&FixtureShape::from_slice(br#"{"note":null}"#).unwrap());
        assert_eq!(added, vec!["note (String)"]);
        assert_eq!(removed, vec!["note (Null)"]);
    }

    #[test]
    fn an_empty_array_records_the_container_but_no_element() {
        assert_eq!(
            shape(r#"{"urls":[]}"#),
            vec![("urls".to_owned(), ValueKind::Array)]
        );
    }
}
