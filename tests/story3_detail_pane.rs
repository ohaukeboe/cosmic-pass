//! Acceptance tests for User Story 3: view item details without leaving the keyboard.

use cosmic_pass::core::state::{Mode, Msg};
use cosmic_pass::model::{FieldRef, ItemKey, ItemKind, ItemSummary};
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

async fn detail(query: &str) -> Harness {
    let backend = FakeBackend::with_items(vec![github(), note()]);
    backend.set_field(&key("gh"), "password", "SECRET-FIXTURE-gh");
    backend.set_totp(&key("gh"), &[("totp_uri", "123456")]);
    let mut h = Harness::new(backend);
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::QueryChanged(query.into())).await;
    h.send(Msg::OpenDetail).await;
    h
}

#[tokio::test]
async fn opening_detail_fetches_totp_only_when_present() {
    let h = detail("github").await;
    assert_eq!(h.model.view.mode, Mode::Detail { key: key("gh") });
    assert!(h.model.view.revealed.is_none());
    assert_eq!(h.backend.totp_calls(), 1);
    let totp = h.model.view.totp.as_ref().unwrap();
    assert_eq!(totp.code.expose_secret(), "123456");
    assert!(totp.valid_until > h.now());

    let h = detail("notes").await;
    assert_eq!(h.model.view.mode, Mode::Detail { key: key("n") });
    assert_eq!(h.backend.totp_calls(), 0);
    assert!(h.model.view.totp.is_none());
}

#[tokio::test]
async fn reveal_toggles() {
    let mut h = detail("github").await;
    h.send(Msg::ToggleReveal).await;
    assert_eq!(
        h.model
            .view
            .revealed
            .as_ref()
            .map(|s| s.expose_secret().to_owned()),
        Some("SECRET-FIXTURE-gh".to_owned())
    );
    h.send(Msg::ToggleReveal).await;
    assert!(h.model.view.revealed.is_none());
}

#[tokio::test]
async fn leaving_drops_secrets() {
    let mut h = detail("github").await;
    h.send(Msg::ToggleReveal).await;
    h.send(Msg::Back).await;
    assert_eq!(h.model.view.mode, Mode::List);
    assert!(h.model.view.revealed.is_none());
    assert!(h.model.view.totp.is_none());

    let mut h = detail("github").await;
    h.send(Msg::ToggleReveal).await;
    h.send(Msg::Hide).await;
    assert!(h.model.view.revealed.is_none());
    assert!(h.model.view.totp.is_none());
}

#[tokio::test]
async fn escape_leaves_detail_without_hiding() {
    let mut h = detail("github").await;
    h.send(Msg::Escape).await;
    assert_eq!(h.model.view.mode, Mode::List);
    assert!(h.window_visible());
}

#[tokio::test]
async fn totp_refreshes_after_expiry() {
    let mut h = detail("github").await;
    let until = h.model.view.totp.as_ref().unwrap().valid_until;
    h.send(Msg::Tick(until - 1)).await;
    assert_eq!(h.backend.totp_calls(), 1);
    h.backend.set_totp(&key("gh"), &[("totp_uri", "654321")]);
    h.advance(until - h.now());
    h.send(Msg::Tick(until)).await;
    assert_eq!(h.backend.totp_calls(), 2);
    assert_eq!(
        h.model.view.totp.as_ref().unwrap().code.expose_secret(),
        "654321"
    );
    h.send(Msg::Tick(until)).await;
    assert_eq!(h.backend.totp_calls(), 2, "no refetch while still valid");
}

#[tokio::test]
async fn copy_chords_work_in_detail() {
    let mut h = detail("github").await;
    h.send(Msg::CopyUsername).await;
    assert_eq!(h.clipboard.copies()[0].0, "octocat");
    assert!(!h.window_visible());
}

#[tokio::test]
async fn enter_in_detail_copies_primary() {
    let mut h = detail("github").await;
    h.send(Msg::CopyPrimary).await;
    assert_eq!(h.clipboard.copies()[0].0, "SECRET-FIXTURE-gh");
}

#[tokio::test]
async fn stale_reveal_result_is_ignored() {
    let mut h = detail("github").await;
    h.backend.set_delay(std::time::Duration::from_millis(100));
    h.send_no_wait(Msg::ToggleReveal);
    h.send(Msg::Back).await;
    assert!(h.model.view.revealed.is_none());
}

#[tokio::test]
async fn detail_pane_lists_each_website_once() {
    // Every website is its own copyable field, so the pane must not also render the extras.
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
    let backend = FakeBackend::with_items(vec![item]);
    let mut h = Harness::new(backend);
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::OpenDetail).await;

    let shown = cosmic_pass::app::view::detail::rows(&h.model, h.now());
    let websites: Vec<_> = shown
        .iter()
        .filter(|(_, value)| value.starts_with("https://"))
        .collect();
    assert_eq!(
        websites.len(),
        2,
        "each website appears exactly once: {shown:?}"
    );
    assert_eq!(websites[0].0, "Website");
    assert_eq!(websites[1].0, "Website 2");
}
