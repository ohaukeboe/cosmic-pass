//! Integration tests for the `pass-cli` boundary, using `tests/fixtures/fake-pass-cli`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cosmic_pass::pass::error::PassError;
use cosmic_pass::pass::runner::{CommandRunner, TokioRunner};
use tokio_util::sync::CancellationToken;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fake_runner(env: &[(&str, &str)]) -> TokioRunner {
    let mut vars: Vec<(String, String)> = vec![(
        "FAKE_FIXTURE_DIR".into(),
        fixtures().join("pass-cli/synthetic").display().to_string(),
    )];
    vars.extend(env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    TokioRunner::new(fixtures().join("fake-pass-cli")).with_env(vars)
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

const LONG: Duration = Duration::from_secs(10);

mod runner {
    use super::*;

    #[tokio::test]
    async fn returns_stdout_on_success() {
        let out = fake_runner(&[])
            .run(
                args(&["vault", "list", "--output", "json"]),
                LONG,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("\"vaults\""));
    }

    #[tokio::test]
    async fn sets_quiet_env_and_null_stdin() {
        let out = fake_runner(&[("FAKE_ECHO_ENV", "1")])
            .run(args(&["info"]), LONG, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "PASS_LOG_LEVEL=off PROTON_PASS_NO_UPDATE_CHECK=1 STDIN=eof"
        );
    }

    #[tokio::test]
    async fn classifies_failures_from_stderr() {
        let err = fake_runner(&[
            ("FAKE_EXIT", "1"),
            (
                "FAKE_STDERR",
                "Error: This operation requires an authenticated client",
            ),
        ])
        .run(args(&["info"]), LONG, CancellationToken::new())
        .await
        .unwrap_err();
        assert_eq!(err, PassError::SignedOut);
    }

    #[tokio::test]
    async fn missing_binary_is_cli_missing() {
        let err = TokioRunner::new("/nonexistent/pass-cli")
            .run(args(&["info"]), LONG, CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(err, PassError::CliMissing);
    }

    fn process_gone(pid: i32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => true,
            // A killed child may linger as a zombie until it is reaped.
            Ok(stat) => stat
                .rsplit(')')
                .next()
                .is_some_and(|rest| rest.trim_start().starts_with('Z')),
        }
    }

    async fn assert_killed(pid_file: &std::path::Path) {
        let pid: i32 = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        for _ in 0..50 {
            if process_gone(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("process {pid} still running");
    }

    #[tokio::test]
    async fn timeout_kills_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let runner = fake_runner(&[
            ("FAKE_SLEEP", "5"),
            ("FAKE_PID_FILE", pid_file.to_str().unwrap()),
        ]);
        let start = Instant::now();
        let err = runner
            .run(
                args(&["info"]),
                Duration::from_millis(500),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(err, PassError::Timeout);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_killed(&pid_file).await;
    }

    #[tokio::test]
    async fn cancel_kills_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let runner = fake_runner(&[
            ("FAKE_SLEEP", "5"),
            ("FAKE_PID_FILE", pid_file.to_str().unwrap()),
        ]);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            trigger.cancel();
        });
        let err = runner.run(args(&["info"]), LONG, cancel).await.unwrap_err();
        assert_eq!(err, PassError::Cancelled);
        assert_killed(&pid_file).await;
    }

    #[tokio::test]
    async fn dropping_the_future_kills_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let runner = fake_runner(&[
            ("FAKE_SLEEP", "5"),
            ("FAKE_PID_FILE", pid_file.to_str().unwrap()),
        ]);
        let fut = runner.run(args(&["info"]), LONG, CancellationToken::new());
        let _ = tokio::time::timeout(Duration::from_millis(300), fut).await;
        assert_killed(&pid_file).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn runs_at_most_four_processes_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("times");
        let runner = std::sync::Arc::new(fake_runner(&[
            ("FAKE_SLEEP", "0.3"),
            ("FAKE_TIMES_LOG", log.to_str().unwrap()),
        ]));
        let tasks: Vec<_> = (0..10)
            .map(|_| {
                let r = runner.clone();
                tokio::spawn(
                    async move { r.run(args(&["info"]), LONG, CancellationToken::new()).await },
                )
            })
            .collect();
        for t in tasks {
            t.await.unwrap().unwrap();
        }
        let mut events: Vec<(u128, i32)> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|l| {
                let (kind, ts) = l.split_once(' ').unwrap();
                (ts.parse().unwrap(), if kind == "start" { 1 } else { -1 })
            })
            .collect();
        events.sort();
        let (mut running, mut peak) = (0, 0);
        for (_, delta) in events {
            running += delta;
            peak = peak.max(running);
        }
        assert!(peak <= 4, "peak concurrency {peak}");
        assert!(peak >= 2, "calls did not run concurrently");
    }
}

mod backend {
    use super::*;
    use cosmic_pass::model::{AccountId, ItemKey};
    use cosmic_pass::pass::backend::{PassBackend, PassCli};
    use secrecy::ExposeSecret;
    use std::sync::Arc;

    fn backend(env: &[(&str, &str)]) -> PassCli<TokioRunner> {
        PassCli::new(Arc::new(fake_runner(env)))
    }

    fn logged(log: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[tokio::test]
    async fn account_when_signed_in() {
        let id = backend(&[]).account().await.unwrap();
        assert_eq!(id, AccountId("account-1".into()));
    }

    #[tokio::test]
    async fn account_when_signed_out() {
        let err = backend(&[
            ("FAKE_EXIT", "1"),
            (
                "FAKE_STDERR",
                "Error: This operation requires an authenticated client",
            ),
        ])
        .account()
        .await
        .unwrap_err();
        assert_eq!(err, PassError::SignedOut);
    }

    #[tokio::test]
    async fn list_all_lists_every_vault_once() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("argv");
        let listing = backend(&[("FAKE_ARGV_LOG", log.to_str().unwrap())])
            .list_all()
            .await
            .unwrap();
        assert_eq!(listing.vaults.len(), 2);
        assert_eq!(listing.items.len(), 11);
        assert!(listing.items.iter().any(|i| i.vault_name == "Work"));

        let mut calls = logged(&log);
        calls.sort();
        assert_eq!(
            calls,
            vec![
                "item list --share-id=-share-b --output json --show-secrets",
                "item list --share-id=share-a --output json --show-secrets",
                "vault list --output json",
            ]
        );
    }

    #[tokio::test]
    async fn list_all_fails_if_any_vault_fails() {
        // share-c has no fixture, so its listing fails.
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            fixtures().join("pass-cli/synthetic/item-list-share-a.json"),
            dir.path().join("item-list-share-a.json"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("vault-list.json"),
            r#"{"vaults":[{"name":"A","vault_id":"a","share_id":"share-a"},
                          {"name":"C","vault_id":"c","share_id":"share-c"}]}"#,
        )
        .unwrap();
        let err = backend(&[("FAKE_FIXTURE_DIR", dir.path().to_str().unwrap())])
            .list_all()
            .await
            .unwrap_err();
        assert_eq!(err, PassError::NotFound);
    }

    #[tokio::test]
    async fn get_field_reads_raw_value() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("argv");
        let value = backend(&[("FAKE_ARGV_LOG", log.to_str().unwrap())])
            .get_field(
                ItemKey::new("-share-b", "login-github-work"),
                "password".into(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(value.expose_secret(), "SECRET-FIXTURE-work-password");
        let calls = logged(&log);
        assert_eq!(
            calls,
            vec!["item view --share-id=-share-b --item-id=login-github-work --field=password"]
        );
        assert!(calls.iter().all(|c| !c.contains("SECRET-FIXTURE-")));
    }

    #[tokio::test]
    async fn get_field_supports_custom_names_with_spaces() {
        let value = backend(&[])
            .get_field(
                ItemKey::new("share-a", "custom-api"),
                "Token".into(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(value.expose_secret(), "SECRET-FIXTURE-api-token");
    }

    #[tokio::test]
    async fn missing_field_is_reported() {
        let err = backend(&[
            ("FAKE_EXIT", "1"),
            ("FAKE_STDERR", "Error: Field does not exist: pin"),
        ])
        .get_field(
            ItemKey::new("share-a", "card-visa"),
            "pin".into(),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(err, PassError::FieldMissing);
    }

    #[tokio::test]
    async fn totp_returns_codes_by_field() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("argv");
        let codes = backend(&[("FAKE_ARGV_LOG", log.to_str().unwrap())])
            .totp(
                ItemKey::new("share-a", "login-github"),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(codes["totp_uri"].expose_secret(), "123456");
        assert_eq!(
            logged(&log),
            vec!["item totp --share-id=share-a --item-id=login-github --output json"]
        );
    }

    #[tokio::test]
    async fn cancelled_field_read() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = backend(&[("FAKE_SLEEP", "5")])
            .get_field(
                ItemKey::new("share-a", "login-github"),
                "password".into(),
                cancel,
            )
            .await
            .unwrap_err();
        assert_eq!(err, PassError::Cancelled);
    }

    #[tokio::test]
    async fn login_streams_output_lines() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        backend(&[("FAKE_LOGIN_URL", "https://example.invalid/login")])
            .login(tx)
            .await
            .unwrap();
        let mut lines = Vec::new();
        while let Some(l) = rx.recv().await {
            lines.push(l);
        }
        assert_eq!(lines, vec!["Please go to https://example.invalid/login"]);
    }

    #[tokio::test]
    async fn login_failure_is_classified() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let err = backend(&[
            ("FAKE_EXIT", "1"),
            ("FAKE_STDERR", "Error: Login cancelled"),
        ])
        .login(tx)
        .await
        .unwrap_err();
        assert_eq!(
            err,
            PassError::Cli {
                message: "Login cancelled".into()
            }
        );
        assert_eq!(rx.recv().await.as_deref(), Some("Error: Login cancelled"));
    }

    #[tokio::test]
    async fn lists_five_thousand_items() {
        let listing = backend(&[("FAKE_ITEMS", "5000")]).list_all().await.unwrap();
        assert_eq!(listing.items.len(), 10_000, "5,000 per vault, two vaults");
        let mut index = cosmic_pass::core::search::SearchIndex::build(&listing.items);
        let rows = index.search("site 4999", |_| None, &listing.items, 50);
        assert!(!rows.is_empty());
        assert!(listing.items.iter().all(|i| {
            i.fields
                .iter()
                .all(|f| f.value.as_deref() != Some("SECRET-FIXTURE-gen"))
        }));
    }
}
