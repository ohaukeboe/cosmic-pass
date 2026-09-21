//! Acceptance tests for User Story 4: recover from a signed-out or unavailable state.

use cosmic_pass::app::view;
use cosmic_pass::core::state::{Msg, SessionState};
use cosmic_pass::model::{AccountId, CliVersion, ItemKind};
use cosmic_pass::pass::error::PassError;
use cosmic_pass::testing::{FakeBackend, Harness, listing, summary};

fn items() -> FakeBackend {
    FakeBackend::with_items(vec![
        summary("a", "Alpha", ItemKind::Login),
        summary("b", "Beta", ItemKind::Login),
    ])
}

async fn started(backend: FakeBackend) -> Harness {
    let mut h = Harness::new(backend);
    h.send(Msg::Startup).await;
    h
}

#[tokio::test]
async fn startup_probes_session_then_refreshes() {
    let h = started(items()).await;
    assert_eq!(
        h.model.session,
        SessionState::SignedIn(AccountId("account-1".into()))
    );
    assert_eq!(h.backend.list_calls(), 1);
    assert_eq!(h.model.data.items.len(), 2);
}

/// A `pass-cli` older than the tested one warns and otherwise changes nothing: the session
/// still signs in, the items still load. Only a warning, never a refusal.
#[tokio::test]
async fn an_old_pass_cli_warns_but_still_works() {
    let backend = items();
    backend.set_version(Ok(CliVersion::new(2, 0, 2)));
    let h = started(backend).await;
    let warning = h.model.cli_warning.as_deref().expect("a warning");
    assert!(warning.contains("2.0.2"), "{warning}");
    assert!(view::notices(&h.model).contains(&warning));
    assert_eq!(
        h.model.session,
        SessionState::SignedIn(AccountId("account-1".into()))
    );
    assert_eq!(h.model.data.items.len(), 2);
}

/// A `pass-cli` that cannot report a version is left to the session probe to explain.
#[tokio::test]
async fn an_unreadable_version_adds_no_line() {
    let backend = items();
    backend.set_version(Err(PassError::Protocol {
        command: "--version",
    }));
    let h = started(backend).await;
    assert_eq!(h.model.cli_warning, None);
    assert_eq!(h.model.data.items.len(), 2);
}

#[tokio::test]
async fn signed_out_shows_status_and_offers_login() {
    let backend = items();
    backend.set_account(Err(PassError::SignedOut));
    let mut h = started(backend).await;
    h.send(Msg::Show).await;
    assert_eq!(h.model.session, SessionState::SignedOut);
    assert!(h.model.can_start_login());
    assert_eq!(h.backend.list_calls(), 0);
    assert!(h.cache_deletions() >= 1);
}

#[tokio::test]
async fn missing_cli_is_reported() {
    let backend = items();
    backend.set_account(Err(PassError::CliMissing));
    let h = started(backend).await;
    assert_eq!(h.model.session, SessionState::CliMissing);
    assert!(!h.model.can_start_login());
    assert!(
        cosmic_pass::core::state::PASS_CLI_URL.starts_with("https://protonpass.github.io/pass-cli")
    );
}

#[tokio::test]
async fn network_error_keeps_items_and_marks_stale() {
    let mut h = started(items()).await;
    h.backend.set_listing(Err(PassError::Network));
    h.send(Msg::RefreshRequested).await;
    assert_eq!(h.model.data.items.len(), 2);
    assert!(h.model.data.stale);
    h.send(Msg::QueryChanged("beta".into())).await;
    assert_eq!(h.result_ids(), ["b"]);
    assert!(matches!(h.model.session, SessionState::SignedIn(_)));
}

#[tokio::test]
async fn network_error_is_reported_as_unreachable() {
    let mut h = started(items()).await;
    h.backend.set_listing(Err(PassError::Network));
    h.send(Msg::RefreshRequested).await;
    let notice = h.model.stale_notice().expect("a status line");
    assert!(notice.contains("reach Proton Pass"), "{notice}");
    assert!(notice.contains("F5"), "{notice}");

    h.backend
        .set_listing(Ok(listing(vec![summary("a", "Alpha", ItemKind::Login)])));
    h.send(Msg::RefreshRequested).await;
    assert_eq!(h.model.stale_notice(), None, "cleared by a good refresh");
}

#[tokio::test]
async fn failing_vault_keeps_old_list() {
    let mut h = started(items()).await;
    h.backend.set_listing(Err(PassError::NotFound));
    h.send(Msg::RefreshRequested).await;
    assert_eq!(h.model.data.items.len(), 2);
    assert!(h.model.data.stale);
}

#[tokio::test]
async fn refresh_reporting_signed_out_clears_items() {
    let mut h = started(items()).await;
    h.backend.set_listing(Err(PassError::SignedOut));
    h.send(Msg::RefreshRequested).await;
    assert_eq!(h.model.session, SessionState::SignedOut);
    assert!(h.model.data.items.is_empty());
    assert!(h.model.view.results.is_empty());
    assert!(h.cache_deletions() >= 1);
}

#[tokio::test]
async fn login_streams_url_then_refreshes() {
    let backend = items();
    backend.set_account(Err(PassError::SignedOut));
    backend.set_login(
        &[
            "Opening browser",
            "Please go to https://account.example.invalid/login?x=1 to continue",
        ],
        Ok(()),
    );
    let mut h = started(backend).await;
    h.backend.set_account(Ok(AccountId("account-1".into())));
    h.send(Msg::StartLogin).await;
    assert_eq!(h.backend.login_calls(), 1);
    assert!(h.messages().iter().any(
        |m| m.contains("LoginLine") && m.contains("https://account.example.invalid/login?x=1")
    ));
    assert!(h.model.login_url.is_none(), "cleared once signed in");
    assert!(matches!(h.model.session, SessionState::SignedIn(_)));
    assert_eq!(h.model.data.items.len(), 2);
}

#[tokio::test]
async fn failed_login_shows_error() {
    let backend = items();
    backend.set_account(Err(PassError::SignedOut));
    backend.set_login(
        &[],
        Err(PassError::Cli {
            message: "login aborted".into(),
        }),
    );
    let mut h = started(backend).await;
    h.send(Msg::StartLogin).await;
    assert!(matches!(h.model.session, SessionState::Error(ref m) if m.contains("login aborted")));
    assert!(h.model.can_start_login());
}

#[tokio::test]
async fn open_refreshes_only_when_stale() {
    let mut h = started(items()).await;
    let calls = h.backend.list_calls();
    h.send(Msg::Show).await;
    assert_eq!(h.backend.list_calls(), calls, "fresh data is not refetched");
    h.send(Msg::Hide).await;
    h.advance(301);
    h.send(Msg::Show).await;
    assert_eq!(h.backend.list_calls(), calls + 1);
}

#[tokio::test]
async fn account_switch_deletes_cache_and_refetches() {
    let mut h = started(items()).await;
    let deletions = h.cache_deletions();
    h.backend.set_account(Ok(AccountId("account-2".into())));
    h.backend
        .set_listing(Ok(listing(vec![summary("c", "Gamma", ItemKind::Note)])));
    h.send(Msg::Startup).await;
    assert_eq!(h.cache_deletions(), deletions + 1);
    assert_eq!(h.result_ids(), ["c"]);
}

#[tokio::test]
async fn locked_session() {
    let backend = items();
    backend.set_account(Err(PassError::Locked));
    let h = started(backend).await;
    assert_eq!(h.model.session, SessionState::Locked);
}

#[tokio::test]
async fn copy_failing_with_signed_out_updates_session() {
    let mut h = started(items()).await;
    h.backend.fail_field(
        &cosmic_pass::model::ItemKey::new("s", "a"),
        "password",
        PassError::SignedOut,
    );
    h.model.data.items[0].fields =
        vec![cosmic_pass::model::FieldRef::secret("password", "Password")];
    h.send(Msg::Show).await;
    h.send(Msg::CopyPrimary).await;
    assert_eq!(h.model.session, SessionState::SignedOut);
}
