//! Acceptance tests for User Story 1 of feature 002: reveal a secret from the field list.
//!
//! Ports the assertions of the removed detail-pane acceptance test that still describe
//! required behavior, re-pointed at the field list, the only per-item screen there is.

use cosmic_pass::core::actions::CopySource;
use cosmic_pass::core::state::{Mode, Msg};
use cosmic_pass::model::{FieldRef, ItemKey, ItemKind, ItemSummary};
use cosmic_pass::pass::error::PassError;
use cosmic_pass::testing::{FakeBackend, Harness, summary};
use secrecy::ExposeSecret;

fn key(id: &str) -> ItemKey {
    ItemKey::new("s", id)
}

fn github() -> ItemSummary {
    let mut i = summary("gh", "GitHub", ItemKind::Login);
    i.username = Some("octocat".into());
    i.fields = vec![
        FieldRef::plain("username", "Username", "octocat".into()),
        FieldRef::secret("password", "Password"),
    ];
    i.totp_fields = vec!["totp_uri".into()];
    i
}

fn note() -> ItemSummary {
    let mut i = summary("n", "Notes", ItemKind::Note);
    i.fields = vec![FieldRef::secret("note", "Note")];
    i
}

/// The field list of the item matching `query`, highlight on the first row.
async fn field_list(query: &str) -> Harness {
    let backend = FakeBackend::with_items(vec![github(), note()]);
    backend.set_field(&key("gh"), "password", "SECRET-FIXTURE-gh");
    backend.set_totp(&key("gh"), &[("totp_uri", "123456")]);
    let mut h = Harness::new(backend);
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::QueryChanged(query.into())).await;
    h.send(Msg::OpenActions).await;
    h
}

fn revealed(h: &Harness) -> Option<String> {
    h.model
        .view
        .revealed
        .value()
        .map(|s| s.expose_secret().to_owned())
}

/// Moves the highlight onto the item's one-time-code row.
async fn highlight_code(h: &mut Harness) {
    let at = h
        .model
        .actions()
        .iter()
        .position(|e| matches!(e.source, CopySource::Totp { .. }))
        .expect("the item has a one-time code");
    for _ in 0..at {
        h.send(Msg::SelectNext).await;
    }
}

#[tokio::test]
async fn opening_the_field_list_fetches_nothing() {
    for query in ["github", "notes"] {
        let h = field_list(query).await;
        assert!(matches!(h.model.view.mode, Mode::Actions { .. }), "{query}");
        assert_eq!(h.backend.totp_calls(), 0, "{query}");
        assert_eq!(h.backend.field_calls(), 0, "{query}");
        assert!(h.model.view.revealed.totp_code().is_none(), "{query}");
        assert!(h.model.view.revealed.value().is_none(), "{query}");
    }
}

#[tokio::test]
async fn reveal_toggles_the_highlighted_row() {
    let mut h = field_list("github").await;
    // The first row is the primary one: the password.
    h.send(Msg::ToggleReveal).await;
    assert_eq!(revealed(&h), Some("SECRET-FIXTURE-gh".to_owned()));
    h.send(Msg::ToggleReveal).await;
    assert!(revealed(&h).is_none());
}

#[tokio::test]
async fn only_one_row_is_revealed_at_a_time() {
    let mut h = field_list("github").await;
    h.send(Msg::ToggleReveal).await;
    assert!(revealed(&h).is_some());
    highlight_code(&mut h).await;
    assert!(
        revealed(&h).is_none(),
        "moving the highlight masks the secret"
    );
    h.send(Msg::ToggleReveal).await;
    assert_eq!(
        h.model
            .revealed_totp()
            .map(|t| t.code.expose_secret().to_owned()),
        Some("123456".to_owned())
    );
    assert!(revealed(&h).is_none(), "and no secret came back with it");
}

#[tokio::test]
async fn a_one_time_code_is_fetched_only_when_its_row_is_revealed() {
    let mut h = field_list("github").await;
    highlight_code(&mut h).await;
    assert_eq!(h.backend.totp_calls(), 0);
    h.send(Msg::ToggleReveal).await;
    assert_eq!(h.backend.totp_calls(), 1);
    let totp = h
        .model
        .view
        .revealed
        .totp_code()
        .expect("a code is on screen");
    assert_eq!(totp.code.expose_secret(), "123456");
    assert!(totp.valid_until > h.now());
}

#[tokio::test]
async fn a_revealed_code_refreshes_when_its_period_ends() {
    let mut h = field_list("github").await;
    highlight_code(&mut h).await;
    h.send(Msg::ToggleReveal).await;
    let until = h
        .model
        .view
        .revealed
        .totp_code()
        .expect("a code")
        .valid_until;
    h.send(Msg::Tick(until - 1)).await;
    assert_eq!(h.backend.totp_calls(), 1);
    h.backend.set_totp(&key("gh"), &[("totp_uri", "654321")]);
    h.advance(until - h.now());
    h.send(Msg::Tick(until)).await;
    assert_eq!(h.backend.totp_calls(), 2);
    assert_eq!(
        h.model
            .view
            .revealed
            .totp_code()
            .expect("a code")
            .code
            .expose_secret(),
        "654321"
    );
    h.send(Msg::Tick(until)).await;
    assert_eq!(h.backend.totp_calls(), 2, "no refetch while still valid");
    // Masking ends the refresh: a code nobody is looking at costs no call.
    h.send(Msg::ToggleReveal).await;
    h.send(Msg::Tick(until + 600)).await;
    assert_eq!(h.backend.totp_calls(), 2);
}

#[tokio::test]
async fn leaving_drops_secrets() {
    for leave in [Msg::Back, Msg::Hide] {
        let mut h = field_list("github").await;
        h.send(Msg::ToggleReveal).await;
        assert!(revealed(&h).is_some());
        h.send(leave.clone()).await;
        assert_eq!(h.model.view.mode, Mode::List, "{leave:?}");
        assert!(revealed(&h).is_none(), "{leave:?}");
        assert!(h.model.view.revealed.totp_code().is_none(), "{leave:?}");
    }
}

#[tokio::test]
async fn escape_leaves_the_field_list_without_hiding() {
    let mut h = field_list("github").await;
    h.send(Msg::ToggleReveal).await;
    h.send(Msg::Escape).await;
    assert_eq!(h.model.view.mode, Mode::List);
    assert!(revealed(&h).is_none());
    assert!(h.window_visible());
}

#[tokio::test]
async fn a_reveal_abandoned_before_it_lands_is_ignored() {
    let mut h = field_list("github").await;
    h.backend.set_delay(std::time::Duration::from_millis(100));
    h.send_no_wait(Msg::ToggleReveal);
    h.send(Msg::Back).await;
    assert!(revealed(&h).is_none());
    assert_eq!(h.model.view.mode, Mode::List);
}

#[tokio::test]
async fn a_failed_reveal_reports_and_leaves_the_row_masked() {
    let backend = FakeBackend::with_items(vec![github()]);
    backend.fail_field(&key("gh"), "password", PassError::FieldMissing);
    let mut h = Harness::new(backend);
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::OpenActions).await;
    h.send(Msg::ToggleReveal).await;
    assert!(revealed(&h).is_none());
    assert!(
        h.model.view.notice.is_some(),
        "the failure is reported on the notice line"
    );
    assert!(h.window_visible());
}

#[tokio::test]
async fn copy_chords_work_in_the_field_list() {
    let mut h = field_list("github").await;
    h.send(Msg::CopyUsername).await;
    assert_eq!(h.clipboard.copies()[0].0, "octocat");
    assert!(!h.window_visible());
}

#[tokio::test]
async fn enter_copies_the_highlighted_row() {
    let mut h = field_list("github").await;
    h.send(Msg::ActivateAction(None)).await;
    assert_eq!(h.clipboard.copies()[0].0, "SECRET-FIXTURE-gh");
}

#[tokio::test]
async fn the_field_list_lists_each_website_once() {
    // Every website is its own copyable field, so the list must not also render the extras.
    let mut item = github();
    item.urls = vec![
        "https://github.com/login".into(),
        "https://gist.github.com".into(),
    ];
    item.fields.push(FieldRef::plain(
        "url",
        "Website",
        "https://github.com/login".into(),
    ));
    item.fields.push(FieldRef::plain(
        "url2",
        "Website 2",
        "https://gist.github.com".into(),
    ));
    let mut h = Harness::new(FakeBackend::with_items(vec![item]));
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::OpenActions).await;

    let websites: Vec<_> = h
        .model
        .actions()
        .into_iter()
        .filter(|e| {
            matches!(&e.source, CopySource::Field(f)
                if f.value.as_deref().is_some_and(|v| v.starts_with("https://")))
        })
        .map(|e| e.label)
        .collect();
    assert_eq!(websites, ["Website", "Website 2"]);
}
