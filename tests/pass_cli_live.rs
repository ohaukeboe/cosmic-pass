//! Drives a real, signed-in `pass-cli` end to end, read-only.
//!
//! Opt-in twice over: every test is `#[ignore]`d *and* returns unless `COSMIC_PASS_LIVE=1`.
//! `--ignored` is a blunt, commonly used flag, and nobody should drive their own vault by
//! running it. `just test-live` sets both.
//!
//! What this suite proves that `tests/pass_cli_contract.rs` cannot: that the JSON upstream
//! actually emits still parses into the app's model. Fixtures cannot prove that -- fixtures are
//! precisely the thing that goes stale.
//!
//! Safety rules, enforced by review and by `scripts/leak-scan.sh`:
//!   * read-only -- only `info`, `vault list`, `item list`, `item view`, `item totp`;
//!   * no secret value is printed, written, asserted on by value, or passed in argv;
//!   * assertions are on parser success, counts, lengths and character classes.

#![allow(clippy::print_stderr)]

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cosmic_pass::model::{ItemSummary, ShareId, Vault};
use cosmic_pass::pass::backend::argv as app_argv;
use cosmic_pass::pass::backend::{Listing, PassBackend, PassCli};
use cosmic_pass::pass::error::PassError;
use cosmic_pass::pass::parse;
use cosmic_pass::pass::runner::{CommandRunner, TokioRunner};
use secrecy::ExposeSecret;
use support::real_cli::{
    FixtureShape, LIST_TIMEOUT, LiveScenario, PROBE_TIMEOUT, RealCli, args, or_skip_live,
};
use tokio_util::sync::CancellationToken;

// -------------------------------------------------------------------------------------------
// Preflight

/// The binary, signed in, or `None` after printing why this test did not run.
///
/// Every test calls this: nextest gives each test its own process, so there is no shared setup
/// to hang the check on, and a per-test check is what makes each scenario's line stand alone.
async fn live() -> Option<&'static RealCli> {
    let cli = or_skip_live()?;
    let backend = backend(cli);
    match backend.account().await {
        Ok(_) => Some(cli),
        Err(PassError::SignedOut) => panic!("not signed in; run: pass-cli login"),
        Err(other) => panic!("pass-cli is not usable: {other}"),
    }
}

/// The app's own backend, over the production environment. This is the only suite that can
/// exercise `PROTON_PASS_LINUX_KEYRING=dbus`, because it is the only one with a session.
fn backend(cli: &RealCli) -> PassCli<TokioRunner> {
    PassCli::new(Arc::new(cli.runner()))
}

async fn listing(cli: &RealCli) -> Listing {
    backend(cli)
        .list_all()
        .await
        .expect("vault list and item list should succeed with a session")
}

fn first_vault(listing: &Listing) -> &Vault {
    listing.vaults.first().expect("at least one vault")
}

// -------------------------------------------------------------------------------------------
// Scenarios

/// `info --output json` still parses into an account id.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn info_parses() {
    let Some(cli) = live().await else { return };
    let account = backend(cli).account().await.expect("info parses");
    // The id itself is account-identifying, so only its emptiness is asserted.
    assert!(!account.0.is_empty(), "info returned an empty account id");
    LiveScenario::covered("info", "a signed-in session").report();
}

/// `vault list --output json` still parses into vaults with usable ids.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn vault_list_parses() {
    let Some(cli) = live().await else { return };
    let out = cli
        .runner()
        .run(
            app_argv::vault_list(),
            LIST_TIMEOUT,
            CancellationToken::new(),
        )
        .await
        .expect("vault list succeeds");
    let vaults = parse::parse_vaults(&out.stdout).expect("vault list parses");
    assert!(!vaults.is_empty(), "the account has no vaults to test with");
    for vault in &vaults {
        assert!(!vault.share_id.0.is_empty(), "a vault has no share id");
    }
    eprintln!("vaults: {}", vaults.len());
    LiveScenario::covered("vault-list", "at least one vault").report();
}

/// `item list --share-id=<S> --output json --show-secrets`, the form the app actually sends.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn item_list_parses_with_secrets() {
    let Some(cli) = live().await else { return };
    let listing = listing(cli).await;
    assert!(!listing.items.is_empty(), "no items to test with");
    for item in &listing.items {
        assert!(
            !item.key.share.0.is_empty() && !item.key.item.0.is_empty(),
            "an item parsed without a usable key"
        );
    }
    eprintln!("items: {}", listing.items.len());
    LiveScenario::covered("item-list-show-secrets", "at least one item").report();
}

/// The plain listing. The app never sends it, but `parse_items` accepts it and the captured
/// fixtures carry it, so it is part of the shape this project depends on.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn item_list_parses_plain() {
    let Some(cli) = live().await else { return };
    let listing = listing(cli).await;
    let vault = first_vault(&listing);
    let out = cli
        .runner()
        .run(
            plain_item_list(&vault.share_id),
            LIST_TIMEOUT,
            CancellationToken::new(),
        )
        .await
        .expect("plain item list succeeds");
    let items = parse::parse_items(&out.stdout, &vault.share_id, &vault.name)
        .expect("the plain listing parses");
    eprintln!("plain items in the first vault: {}", items.len());
    LiveScenario::covered("item-list-plain", "at least one vault").report();
}

/// `item view --field=password` still returns a raw value the app can wrap.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn field_view_parses() {
    let Some(cli) = live().await else { return };
    let listing = listing(cli).await;
    let Some(item) = listing
        .items
        .iter()
        .find(|item| item.field("password").is_some_and(|f| f.secret))
        .cloned()
    else {
        LiveScenario::not_covered(
            "item-view-password",
            "an item with a password field",
            "no item on this account has a password field",
        )
        .report();
        return;
    };

    let value = backend(cli)
        .get_field(
            item.key.clone(),
            "password".into(),
            CancellationToken::new(),
        )
        .await
        .expect("item view --field=password parses");
    // Length only. Never the value, and never `ExposeSecret` into a message.
    assert!(
        !value.expose_secret().is_empty(),
        "item view returned an empty password"
    );
    LiveScenario::covered("item-view-password", "an item with a password field").report();
}

/// `item totp --output json` still returns codes keyed by field name.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn totp_parses() {
    let Some(cli) = live().await else { return };
    let listing = listing(cli).await;
    let Some(item) = listing
        .items
        .iter()
        .find(|item| !item.totp_fields.is_empty())
        .cloned()
    else {
        LiveScenario::not_covered(
            "item-totp",
            "an item with a TOTP field",
            "no item on this account has a TOTP field",
        )
        .report();
        return;
    };

    let codes = backend(cli)
        .totp(item.key.clone(), CancellationToken::new())
        .await
        .expect("item totp parses");
    assert!(!codes.is_empty(), "item totp returned no codes");
    for (name, code) in &codes {
        // A character class, not the code: six ASCII digits is the shape the app's countdown
        // and clipboard handling assume.
        let code = code.expose_secret();
        assert!(
            code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()),
            "the code for field {name:?} is {} characters and not all digits",
            code.len()
        );
    }
    eprintln!("totp fields: {}", codes.len());
    LiveScenario::covered("item-totp", "an item with a TOTP field").report();
}

/// The 2.1.4 regression: a field inside a named section is addressed as `Section.Name`.
///
/// The single highest-value scenario here. When upstream changed this, the app silently stopped
/// copying those fields, and no fixture-based test could have noticed.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn field_inside_a_section_parses() {
    let Some(cli) = live().await else { return };
    let listing = listing(cli).await;
    let Some((item, field)) = section_field(&listing) else {
        LiveScenario::not_covered(
            "item-view-section-field",
            "an item with a field inside a named section",
            "no item on this account has a section field",
        )
        .report();
        return;
    };

    let value = backend(cli)
        .get_field(item.key.clone(), field.clone(), CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "pass-cli could not address a field inside a section as Section.Name ({e}); \
                 this is the 2.1.4 breakage returning"
            )
        });
    assert!(
        !value.expose_secret().is_empty(),
        "the section field returned an empty value"
    );
    LiveScenario::covered(
        "item-view-section-field",
        "an item with a field inside a named section",
    )
    .report();
}

/// A field name the app built as `Section.Name`, with its item.
fn section_field(listing: &Listing) -> Option<(&ItemSummary, String)> {
    listing.items.iter().find_map(|item| {
        item.fields
            .iter()
            .find(|f| f.name.contains('.') && f.secret)
            .map(|f| (item, f.name.clone()))
    })
}

/// `NotFound`, which is unreachable without a session: 2.3.3 checks authentication before
/// argument validity, so an unauthenticated run answers `SignedOut` instead.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn malformed_share_id_is_not_found() {
    let Some(cli) = live().await else { return };
    let result = cli
        .runner()
        .run(
            app_argv::item_list("bogus"),
            LIST_TIMEOUT,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result.err(),
        Some(PassError::NotFound),
        "pass-cli's wording for an unusable share id changed, so the app would show a raw \
         error instead of \"item or vault not found\""
    );
    LiveScenario::covered("classify-not-found", "a signed-in session").report();
}

/// `FieldMissing`, likewise only reachable with a session.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn missing_field_is_field_missing() {
    let Some(cli) = live().await else { return };
    let listing = listing(cli).await;
    let Some(item) = listing.items.first() else {
        LiveScenario::not_covered(
            "classify-field-missing",
            "at least one item",
            "the account has no items",
        )
        .report();
        return;
    };
    let result = backend(cli)
        .get_field(
            item.key.clone(),
            "definitely-not-a-field".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result.err(),
        Some(PassError::FieldMissing),
        "pass-cli's wording for a missing field changed, so the app would show a raw error \
         instead of \"this item has no such field\""
    );
    LiveScenario::covered("classify-field-missing", "at least one item").report();
}

/// Reported, never asserted: the figures depend on network and vault size, so an assertion
/// here would be a flake generator rather than a signal.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn latency_is_reported() {
    let Some(cli) = live().await else { return };
    let runner = cli.runner();

    for (name, argv, timeout) in [
        ("info", app_argv::info(), PROBE_TIMEOUT),
        ("vault list", app_argv::vault_list(), LIST_TIMEOUT),
    ] {
        let started = Instant::now();
        runner
            .run(argv, timeout, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{name} failed: {e}"));
        report_latency(name, started.elapsed());
    }

    let listing = listing(cli).await;
    let started = Instant::now();
    for vault in &listing.vaults {
        runner
            .run(
                app_argv::item_list(&vault.share_id.0),
                LIST_TIMEOUT,
                CancellationToken::new(),
            )
            .await
            .expect("item list succeeds");
    }
    report_latency("item list --show-secrets (all vaults)", started.elapsed());
    LiveScenario::covered("latency", "a signed-in session").report();
}

fn report_latency(name: &str, taken: Duration) {
    eprintln!("LATENCY {name}: {:.2}s", taken.as_secs_f64());
}

// -------------------------------------------------------------------------------------------
// Fixture shapes

/// The committed fixtures parse (`src/pass/parse.rs` proves that); nothing proves they still
/// resemble what upstream emits. This does, by key path and value kind only -- never a value,
/// which would be meaningless across accounts and unsafe to write down.
#[tokio::test]
#[ignore = "needs a signed-in pass-cli; run: just test-live"]
async fn committed_fixtures_still_match_reality() {
    let Some(cli) = live().await else { return };
    let runner = cli.runner();
    let listing = listing(cli).await;
    let vault = first_vault(&listing);

    let mut pairs = vec![
        ("info.json", app_argv::info()),
        ("vault-list.json", app_argv::vault_list()),
        (
            "item-list-share-1.json",
            app_argv::item_list(&vault.share_id.0),
        ),
        (
            "item-list-plain-share-1.json",
            plain_item_list(&vault.share_id),
        ),
    ];

    // The `-share-2` fixtures were captured from a second vault. Comparing them against the
    // first vault's listing would say nothing: both fixtures and freshness are per-vault, and
    // which optional sub-objects appear follows the items that vault holds.
    let second = listing.vaults.get(1);
    if let Some(second) = second {
        pairs.push((
            "item-list-share-2.json",
            app_argv::item_list(&second.share_id.0),
        ));
        pairs.push((
            "item-list-plain-share-2.json",
            plain_item_list(&second.share_id),
        ));
    }

    let mut checked = 0;
    for (fixture, argv) in pairs {
        let out = runner
            .run(argv, LIST_TIMEOUT, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{fixture}: the command behind it failed: {e}"));
        let fresh = FixtureShape::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{fixture}: fresh output is not JSON: {e}"));
        compare(fixture, &fresh);
        checked += 1;
    }
    eprintln!("fixtures compared: {checked}");
    LiveScenario::covered("fixture-shape", "a signed-in session").report();

    // Reported as its own scenario rather than folded into the one above: an account with one
    // vault leaves two committed fixtures unchecked, and that must be visible in the coverage
    // report rather than hidden behind a green run (FR-017).
    match second {
        Some(_) => LiveScenario::covered("fixture-shape-share-2", "a second vault").report(),
        None => LiveScenario::not_covered(
            "fixture-shape-share-2",
            "a second vault",
            "the account has one vault, so item-list-share-2.json and \
             item-list-plain-share-2.json were compared against nothing",
        )
        .report(),
    }
}

fn compare(fixture: &str, fresh: &FixtureShape) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pass-cli/captured")
        .join(fixture);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let committed =
        FixtureShape::from_slice(&bytes).unwrap_or_else(|e| panic!("{fixture} is not JSON: {e}"));

    // A fixture holds a sample of items, so it carries only the kinds that vault happened to
    // hold. A kind absent from fresh output is a fact about the account, not about upstream.
    let committed = drop_absent_kinds(&committed, fresh);
    let (missing, added) = committed.diff(fresh);

    if !added.is_empty() {
        // Informational: upstream adding a field breaks nothing today, but the fixture no
        // longer shows the app's parsers everything they might meet.
        eprintln!("FIXTURE {fixture}: upstream added {added:?}");
    }
    assert!(
        missing.is_empty(),
        "FIXTURE {fixture}: upstream no longer emits {missing:?}, so this fixture is fiction \
         and the parser tests built on it prove less than they appear to. Refresh with \
         `just capture-fixtures`."
    );
}

/// Drops fixture paths under an item kind that fresh output does not carry.
fn drop_absent_kinds(committed: &FixtureShape, fresh: &FixtureShape) -> FixtureShape {
    const KINDS: &str = "items[].content.content.";
    let fresh_kinds: BTreeSet<&str> = fresh
        .paths
        .iter()
        .filter_map(|(path, _)| kind_of(path, KINDS))
        .collect();
    FixtureShape {
        paths: committed
            .paths
            .iter()
            .filter(|(path, _)| match kind_of(path, KINDS) {
                Some(kind) => fresh_kinds.contains(kind),
                None => true,
            })
            .cloned()
            .collect(),
    }
}

fn kind_of<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)?.split('.').next()
}

/// The listing without `--show-secrets`. Not sourced from `backend::argv` because the app never
/// sends it -- only the captured fixtures carry this form.
fn plain_item_list(share: &ShareId) -> Vec<String> {
    let mut argv = args(&["item", "list"]);
    argv.push(format!("--share-id={}", share.0));
    argv.extend(args(&["--output", "json"]));
    argv
}
