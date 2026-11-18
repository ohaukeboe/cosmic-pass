//! `serve` driven in-process through its real loop, with the fake backend and clipboard.
//! Contract: `specs/004-serve-mode/contracts/serve-protocol.md`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cosmic_pass::config::Preferences;
use cosmic_pass::model::{AccountId, CliVersion, FieldRef, ItemKey, ItemKind, ItemSummary};
use cosmic_pass::pass::error::PassError;
use cosmic_pass::runtime::Deps;
use cosmic_pass::serve::{MAX_RESULTS, run};
use cosmic_pass::testing::{FakeBackend, FakeClipboard, summary};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};
use tokio::task::JoinHandle;

const NOW: i64 = 1_000_000;
const PASSWORD: &str = "SECRET-FIXTURE-gh";
const PIN: &str = "SECRET-FIXTURE-pin";
const CODE: &str = "424242";

fn key(id: &str) -> ItemKey {
    ItemKey::new("s", id)
}

fn login(id: &str, title: &str, user: &str) -> ItemSummary {
    let mut item = summary(id, title, ItemKind::Login);
    item.username = Some(user.into());
    item.subtitle = Some(user.into());
    item.fields = vec![
        FieldRef::plain("username", "Username", user.into()),
        FieldRef::secret("password", "Password"),
        FieldRef::secret("pin", "PIN"),
    ];
    item.totp_fields = vec!["totp_uri".into()];
    item
}

fn backend() -> FakeBackend {
    let backend = FakeBackend::with_items(vec![
        login("gh", "GitHub", "octocat"),
        login("bank", "Bank", "me"),
    ]);
    backend.set_field(&key("gh"), "password", PASSWORD);
    backend.set_field(&key("gh"), "pin", PIN);
    backend.set_totp(&key("gh"), &[("totp_uri", CODE)]);
    backend
}

/// A running `serve` loop and both ends of its pipes.
struct Serve {
    input: DuplexStream,
    output: Lines<BufReader<DuplexStream>>,
    clipboard: Arc<FakeClipboard>,
    task: JoinHandle<std::io::Result<()>>,
    /// Every snapshot read so far, to check the guarantees over all of them.
    seen: Vec<Value>,
}

impl Serve {
    fn start(backend: FakeBackend) -> Self {
        Self::with_backend(Arc::new(backend))
    }

    fn with_backend(backend: Arc<FakeBackend>) -> Self {
        let (input, their_input) = tokio::io::duplex(1 << 16);
        let (their_output, output) = tokio::io::duplex(1 << 20);
        let clipboard = Arc::new(FakeClipboard::default());
        let deps = Deps {
            backend,
            clipboard: clipboard.clone(),
            cache: None,
            save_preferences: false,
        };
        let prefs = Preferences {
            max_results: MAX_RESULTS,
            ..Preferences::default()
        };
        let task = tokio::spawn(run(
            BufReader::new(their_input),
            their_output,
            deps,
            prefs,
            || NOW,
        ));
        Self {
            input,
            output: BufReader::new(output).lines(),
            clipboard,
            task,
            seen: Vec::new(),
        }
    }

    async fn send(&mut self, line: &str) {
        self.input.write_all(line.as_bytes()).await.unwrap();
        self.input.write_all(b"\n").await.unwrap();
    }

    async fn next(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.output.next_line())
            .await
            .expect("no snapshot within 5 s")
            .unwrap()
            .expect("serve closed its output");
        let snapshot: Value = serde_json::from_str(&line).unwrap();
        self.seen.push(snapshot.clone());
        snapshot
    }

    /// Reads snapshots until one satisfies `pred`.
    async fn until(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..100 {
            let s = self.next().await;
            if pred(&s) {
                return s;
            }
        }
        panic!("no snapshot where {what}; last: {:?}", self.seen.last());
    }

    async fn loaded(backend: FakeBackend) -> Self {
        let mut serve = Self::start(backend);
        serve
            .until("items are listed", |s| {
                s["session"] == "signed_in" && !s["rows"].as_array().unwrap().is_empty()
            })
            .await;
        serve.send(r#"{"t":"show"}"#).await;
        serve.until("visible", |s| s["visible"] == true).await;
        serve
    }

    /// Guarantees 1 and 3 of the contract over every snapshot read.
    fn assert_guarantees(&self) {
        for s in &self.seen {
            let shown = s["fields"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["secret"] == true && (!f["value"].is_null() || !f["totp"].is_null()))
                .count();
            assert!(shown <= 1, "two secrets shown at once: {s}");
        }
        for (copied, _, _) in self.clipboard.copies() {
            for s in &self.seen {
                assert!(
                    !s.to_string().contains(&copied),
                    "copied value in a snapshot: {s}"
                );
            }
        }
    }
}

fn titles(s: &Value) -> Vec<&str> {
    s["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_first_snapshot_comes_before_any_intent() {
    let mut serve = Serve::start(backend());
    let first = serve.next().await;
    assert_eq!(first["v"], 1);
    assert_eq!(first["visible"], false);
    assert_eq!(first["mode"], "list");
}

#[tokio::test]
async fn us1_query_then_copy_closes_and_copies_the_password() {
    let mut serve = Serve::loaded(backend()).await;
    serve.send(r#"{"t":"query","text":"git"}"#).await;
    let s = serve
        .until("the query applies", |s| s["query"] == "git")
        .await;
    assert_eq!(titles(&s), ["GitHub"]);
    assert_eq!(s["rows"][0]["subtitle"], "octocat");
    assert_eq!(s["rows"][0]["vault"], "Personal");
    assert_eq!(s["rows"][0]["kind"], "login");
    serve.send(r#"{"t":"copy_primary"}"#).await;
    serve
        .until("the popup closes", |s| s["visible"] == false)
        .await;
    let copies = serve.clipboard.copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].0, PASSWORD);
    assert!(copies[0].1, "copied as a secret");
    assert_eq!(copies[0].2, Duration::from_secs(90));
    serve.assert_guarantees();
}

#[tokio::test]
async fn us1_selection_moves_and_escape_hides() {
    let mut serve = Serve::loaded(backend()).await;
    serve.send(r#"{"t":"next"}"#).await;
    serve.until("second row", |s| s["selected"] == 1).await;
    serve.send(r#"{"t":"select","index":0}"#).await;
    serve.until("first row", |s| s["selected"] == 0).await;
    serve.send(r#"{"t":"escape"}"#).await;
    serve.until("hidden", |s| s["visible"] == false).await;
}

#[tokio::test]
async fn empty_query_lists_at_most_fifty_rows() {
    let items: Vec<_> = (0..60)
        .map(|i| login(&i.to_string(), &format!("item {i:02}"), "u"))
        .collect();
    let serve = Serve::loaded(FakeBackend::with_items(items)).await;
    let rows = serve.seen.last().unwrap()["rows"].as_array().unwrap().len();
    assert_eq!(rows, usize::from(MAX_RESULTS));
}

#[tokio::test]
async fn us2_fields_reveal_one_secret_and_drop_it_on_move() {
    let mut serve = Serve::loaded(backend()).await;
    serve.send(r#"{"t":"query","text":"git"}"#).await;
    serve
        .until("GitHub first", |s| s["rows"][0]["title"] == "GitHub")
        .await;
    serve.send(r#"{"t":"open_fields"}"#).await;
    let s = serve.until("fields mode", |s| s["mode"] == "fields").await;
    assert_eq!(s["item"]["title"], "GitHub");
    let labels: Vec<_> = s["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["label"].as_str().unwrap().to_owned())
        .collect();
    let at = |label: &str| labels.iter().position(|l| l == label).unwrap();
    let username = &s["fields"][at("Username")];
    assert_eq!(username["secret"], false);
    assert_eq!(username["value"], "octocat");
    assert!(s["fields"][at("Password")]["value"].is_null());

    serve
        .send(&format!(r#"{{"t":"select","index":{}}}"#, at("PIN")))
        .await;
    serve.send(r#"{"t":"reveal"}"#).await;
    let s = serve
        .until("the PIN is shown", |s| {
            s["fields"]
                .as_array()
                .is_some_and(|f| f.iter().any(|f| f["value"] == PIN))
        })
        .await;
    assert_eq!(s["selected"], at("PIN"));

    serve.send(r#"{"t":"prev"}"#).await;
    serve
        .until("the PIN is masked again", |s| !s.to_string().contains(PIN))
        .await;

    serve
        .send(&format!(r#"{{"t":"activate","index":{}}}"#, at("Password")))
        .await;
    serve.until("closes", |s| s["visible"] == false).await;
    assert_eq!(serve.clipboard.copies().len(), 1);
    serve.assert_guarantees();
}

#[tokio::test]
async fn us2_a_revealed_code_carries_its_seconds_left() {
    let mut serve = Serve::loaded(backend()).await;
    serve.send(r#"{"t":"query","text":"git"}"#).await;
    serve
        .until("GitHub first", |s| s["rows"][0]["title"] == "GitHub")
        .await;
    serve.send(r#"{"t":"open_fields"}"#).await;
    let s = serve.until("fields", |s| s["mode"] == "fields").await;
    let code_row = s["fields"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| f["label"].as_str().unwrap().contains("code"))
        .expect("a one-time code row");
    serve
        .send(&format!(r#"{{"t":"select","index":{code_row}}}"#))
        .await;
    serve.send(r#"{"t":"reveal"}"#).await;
    let s = serve
        .until("the code is shown", |s| {
            !s["fields"][code_row]["totp"].is_null()
        })
        .await;
    assert_eq!(s["fields"][code_row]["totp"]["code"], CODE);
    assert!(s["fields"][code_row]["totp"]["remaining"].as_u64().unwrap() >= 1);
    serve.send(r#"{"t":"escape"}"#).await;
    serve
        .until("back in the list, code gone", |s| {
            s["mode"] == "list" && !s.to_string().contains(CODE)
        })
        .await;
    serve.assert_guarantees();
}

#[tokio::test]
async fn us3_signed_out_offers_sign_in_and_shows_its_lines() {
    let backend = Arc::new(backend());
    backend.set_account(Err(PassError::SignedOut));
    backend.set_login(
        &["Open https://account.proton.me/x to sign in", "waiting"],
        Ok(()),
    );
    let mut serve = Serve::with_backend(backend.clone());
    serve
        .until("signed out", |s| {
            s["session"] == "signed_out" && s["can_login"] == true
        })
        .await;
    backend.set_account(Ok(AccountId("account-1".into())));
    serve.send(r#"{"t":"login"}"#).await;
    let lines = serve
        .until("sign-in lines arrive", |s| {
            s["login"]["lines"].as_array().is_some_and(|l| l.len() == 2)
        })
        .await;
    assert_eq!(lines["session"], "logging_in");
    assert_eq!(lines["login"]["url"], "https://account.proton.me/x");
    serve
        .until("items after sign-in", |s| {
            s["session"] == "signed_in" && s["login"].is_null()
        })
        .await;
}

#[tokio::test]
async fn us3_a_failed_sign_in_is_named_and_can_be_retried() {
    let backend = Arc::new(backend());
    backend.set_account(Err(PassError::SignedOut));
    backend.set_login(&[], Err(PassError::Timeout));
    let mut serve = Serve::with_backend(backend);
    serve.until("signed out", |s| s["can_login"] == true).await;
    serve.send(r#"{"t":"login"}"#).await;
    let s = serve
        .until("the failure", |s| s["session"] == "error")
        .await;
    assert!(!s["session_detail"].is_null());
    assert_eq!(s["can_login"], true);
}

#[tokio::test]
async fn us3_each_failure_state_is_named() {
    let cases: [(PassError, &str); 3] = [
        (PassError::Locked, "locked"),
        (PassError::CliMissing, "cli_missing"),
        (PassError::LocalData, "error"),
    ];
    for (error, session) in cases {
        let backend = backend();
        backend.set_account(Err(error.clone()));
        let mut serve = Serve::start(backend);
        let s = serve.until(session, |s| s["session"] == session).await;
        if session == "error" {
            assert!(
                s["session_detail"]
                    .as_str()
                    .unwrap()
                    .contains("pass-cli logout --force"),
                "names the fix"
            );
        }
    }
}

#[tokio::test]
async fn us3_out_of_date_items_say_so() {
    let backend = Arc::new(backend());
    let mut serve = Serve::with_backend(backend.clone());
    serve
        .until("items", |s| !s["rows"].as_array().unwrap().is_empty())
        .await;
    backend.set_listing(Err(PassError::Network));
    serve.send(r#"{"t":"refresh"}"#).await;
    let s = serve.until("stale", |s| !s["stale"].is_null()).await;
    assert!(!s["rows"].as_array().unwrap().is_empty(), "keeps the items");
}

#[tokio::test]
async fn us3_an_old_cli_warns_and_a_failed_copy_is_noticed() {
    let backend = backend();
    backend.set_version(Ok(CliVersion::new(2, 0, 0)));
    let mut serve = Serve::loaded(backend).await;
    assert!(serve.seen.iter().any(|s| !s["cli_warning"].is_null()));
    serve.clipboard.fail();
    serve.send(r#"{"t":"copy_primary"}"#).await;
    let s = serve.until("a notice", |s| !s["notice"].is_null()).await;
    assert_eq!(s["visible"], true, "stays open on failure");
}

#[tokio::test]
async fn a_garbage_line_is_skipped_and_eof_ends_cleanly() {
    let mut serve = Serve::start(backend());
    serve.next().await;
    serve.send("not json").await;
    serve.send(r#"{"t":"nope"}"#).await;
    serve.send("").await;
    serve.send(r#"{"t":"show"}"#).await;
    serve.until("still serving", |s| s["visible"] == true).await;
    drop(serve.input);
    let ended = tokio::time::timeout(Duration::from_secs(5), serve.task)
        .await
        .expect("serve did not stop at end of input")
        .unwrap();
    assert!(ended.is_ok());
}

#[test]
fn an_out_of_range_timeout_exits_2_before_any_output() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cosmic-pass"))
        .args(["serve", "--clipboard-clear-secs", "5"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

/// SC-001: with 2,000 items, a query's snapshot within 100 ms. Release only, through the
/// `just bench` filter, like `tests/search_bench.rs`.
#[tokio::test]
#[ignore = "timing; run with `just bench`"]
async fn search_bench_serve_query() {
    let items: Vec<_> = (0..2_000)
        .map(|i| login(&i.to_string(), &format!("account number {i:04}"), "user"))
        .collect();
    let mut serve = Serve::loaded(FakeBackend::with_items(items)).await;
    let started = Instant::now();
    serve.send(r#"{"t":"query","text":"nmbr 19"}"#).await;
    serve.until("the query", |s| s["query"] == "nmbr 19").await;
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "query snapshot took {elapsed:?}"
    );
}

/// The GitHub item highlighted, as the copy chords of specs/017 (dot-emacs) need.
async fn github_highlighted() -> Serve {
    let mut serve = Serve::loaded(backend()).await;
    serve.send(r#"{"t":"query","text":"git"}"#).await;
    serve
        .until("GitHub first", |s| s["rows"][0]["title"] == "GitHub")
        .await;
    serve
}

#[tokio::test]
async fn copy_username_and_code_close_like_enter() {
    for (intent, value) in [("copy_username", "octocat"), ("copy_totp", CODE)] {
        let mut serve = github_highlighted().await;
        serve.send(&format!(r#"{{"t":"{intent}"}}"#)).await;
        serve.until("closes", |s| s["visible"] == false).await;
        let copies = serve.clipboard.copies();
        assert_eq!(copies.len(), 1, "{intent}");
        assert_eq!(copies[0].0, value, "{intent}");
        // The username is drawn in the row anyway; only the code is a guarded copy.
        if intent == "copy_totp" {
            serve.assert_guarantees();
        }
    }
}

#[tokio::test]
async fn copy_username_in_fields_mode_ignores_the_highlight() {
    let mut serve = github_highlighted().await;
    serve.send(r#"{"t":"open_fields"}"#).await;
    serve.until("fields", |s| s["mode"] == "fields").await;
    serve.send(r#"{"t":"copy_username"}"#).await;
    serve.until("closes", |s| s["visible"] == false).await;
    assert_eq!(serve.clipboard.copies()[0].0, "octocat");
}

#[tokio::test]
async fn copy_url_without_a_website_says_so_and_stays_open() {
    let mut serve = github_highlighted().await;
    serve.send(r#"{"t":"copy_url"}"#).await;
    let s = serve.until("a notice", |s| !s["notice"].is_null()).await;
    assert_eq!(s["visible"], true);
    assert!(serve.clipboard.copies().is_empty());
}

#[tokio::test]
async fn retry_checks_the_session_again() {
    let backend = Arc::new(backend());
    backend.set_account(Err(PassError::Locked));
    let mut serve = Serve::with_backend(backend.clone());
    serve.until("locked", |s| s["session"] == "locked").await;
    backend.set_account(Ok(AccountId("account-1".into())));
    serve.send(r#"{"t":"retry"}"#).await;
    serve
        .until("signed in", |s| s["session"] == "signed_in")
        .await;
}
