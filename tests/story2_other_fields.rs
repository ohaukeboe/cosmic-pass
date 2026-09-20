//! Acceptance tests for User Story 2: copy other fields and one-time codes.

use std::time::Duration;

use cosmic_pass::core::state::{Mode, Msg};
use cosmic_pass::model::{FieldRef, ItemKey, ItemKind, ItemSummary, ShareId};
use cosmic_pass::pass::parse::parse_items;
use cosmic_pass::testing::{FakeBackend, Harness, summary};

fn key(id: &str) -> ItemKey {
    ItemKey::new("s", id)
}

fn with_totp() -> ItemSummary {
    let mut i = summary("gh", "GitHub", ItemKind::Login);
    i.username = Some("octocat".into());
    i.urls = vec![
        "https://github.com/login".into(),
        "https://gist.github.com".into(),
    ];
    i.fields = vec![
        FieldRef::plain("username", "Username", "octocat".into()),
        FieldRef::secret("password", "Password"),
        FieldRef::plain("url", "Website", "https://github.com/login".into()),
        FieldRef::plain("url2", "Website 2", "https://gist.github.com".into()),
    ];
    i.totp_fields = vec!["totp_uri".into(), "Backup".into()];
    i
}

fn email_only() -> ItemSummary {
    let mut i = summary("mail", "Mail", ItemKind::Login);
    i.email = Some("me@example.invalid".into());
    i.fields = vec![
        FieldRef::plain("email", "Email", "me@example.invalid".into()),
        FieldRef::secret("password", "Password"),
    ];
    i
}

fn card() -> ItemSummary {
    let mut i = summary("visa", "Visa", ItemKind::CreditCard);
    i.fields = vec![
        FieldRef::secret("number", "Card number"),
        FieldRef::plain("cardholder_name", "Cardholder", "Fixture Holder".into()),
        FieldRef::secret("verification_number", "Security code"),
    ];
    i
}

/// A login as `pass-cli` prints it, carrying a user-defined text field. Parsed rather than
/// hand-built, because the parser is the one place such a field is dropped.
fn with_custom_text() -> ItemSummary {
    let json = br#"{"items":[{"id":"nick","state":"Active","content":{"title":"Nickname Co",
      "note":"","content":{"Login":{"username":"ada","password":"SECRET-FIXTURE-nick",
      "urls":["https://nick.example"],"totp_uri":"otpauth://totp/nick"}},
      "extra_fields":[{"name":"Nickname","content":{"Text":"fix"}}]}}]}"#;
    parse_items(json, &ShareId("s".into()), "Personal")
        .expect("the fixture parses")
        .remove(0)
}

async fn opened(query: &str) -> Harness {
    let backend =
        FakeBackend::with_items(vec![with_totp(), email_only(), card(), with_custom_text()]);
    backend.set_totp(&key("nick"), &[("totp_uri", "654321")]);
    backend.set_totp(
        &key("gh"),
        &[
            ("totp", "111111"),
            ("totp_uri", "111111"),
            ("Backup", "222222"),
        ],
    );
    backend.set_field(&key("visa"), "verification_number", "SECRET-FIXTURE-cvv");
    let mut h = Harness::new(backend);
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::QueryChanged(query.into())).await;
    h
}

fn last_copy(h: &Harness) -> Option<(String, bool)> {
    h.clipboard.copies().last().map(|(v, s, _)| (v.clone(), *s))
}

fn notice(h: &Harness) -> Option<&str> {
    h.model.view.notice.as_ref().map(|n| n.text.as_str())
}

#[tokio::test]
async fn copy_username() {
    let mut h = opened("github").await;
    h.send(Msg::CopyUsername).await;
    assert_eq!(last_copy(&h), Some(("octocat".into(), false)));
    assert!(!h.window_visible());
}

#[tokio::test]
async fn copy_username_falls_back_to_email() {
    let mut h = opened("mail").await;
    h.send(Msg::CopyUsername).await;
    assert_eq!(last_copy(&h), Some(("me@example.invalid".into(), false)));
}

#[tokio::test]
async fn copy_totp() {
    let mut h = opened("github").await;
    h.send(Msg::CopyTotp).await;
    assert_eq!(last_copy(&h), Some(("111111".into(), true)));
    assert_eq!(h.backend.totp_calls(), 1);
    assert!(!h.window_visible());
}

#[tokio::test]
async fn copy_totp_without_code_shows_notice() {
    let mut h = opened("mail").await;
    h.send(Msg::CopyTotp).await;
    assert!(h.clipboard.copies().is_empty());
    assert_eq!(notice(&h), Some("This item has no one-time code"));
    assert!(h.window_visible());
    assert_eq!(h.backend.totp_calls(), 0);
}

#[tokio::test]
async fn the_action_list_shows_why_a_copy_did_nothing() {
    // Acceptance scenario 4: the message has to reach the pane the user is looking at.
    let mut h = opened("mail").await;
    h.send(Msg::OpenActions).await;
    h.send(Msg::CopyTotp).await;
    assert!(h.clipboard.copies().is_empty());
    assert!(
        cosmic_pass::app::view::notices(&h.model).contains(&"This item has no one-time code"),
        "the action pane captions the notice"
    );
}

/// A website is re-read on copy like any other field, so a URL edited in Proton Pass since
/// the last refresh is not copied stale (cosmic-pass-wqx.34).
#[tokio::test]
async fn copy_url_fetches_the_current_value() {
    let mut h = opened("github").await;
    h.send(Msg::CopyUrl).await;
    assert_eq!(
        last_copy(&h),
        Some(("https://github.com/login".into(), false))
    );
    assert_eq!(h.backend.field_calls(), 1);
    assert_eq!(h.backend.totp_calls(), 0);
    assert_eq!(
        h.clipboard.copies()[0].2,
        Duration::from_secs(90),
        "timeout value is passed but the clipboard ignores it for non-secret copies"
    );
}

#[tokio::test]
async fn action_list_copies_highlighted_field() {
    let mut h = opened("visa").await;
    h.send(Msg::OpenActions).await;
    assert!(
        matches!(h.model.view.mode, Mode::Actions { ref key, selected: 0 } if *key == self::key("visa"))
    );
    assert_eq!(h.model.actions().len(), 3);
    h.send(Msg::SelectNext).await;
    h.send(Msg::SelectNext).await;
    h.send(Msg::ActivateAction(None)).await;
    assert_eq!(last_copy(&h), Some(("SECRET-FIXTURE-cvv".into(), true)));
    assert!(!h.window_visible());
}

#[tokio::test]
async fn action_list_back_keeps_selection() {
    let mut h = opened("").await;
    h.send(Msg::SelectNext).await;
    let selected = h.model.view.selected;
    h.send(Msg::OpenActions).await;
    h.send(Msg::Back).await;
    assert_eq!(h.model.view.mode, Mode::List);
    assert_eq!(h.model.view.selected, selected);
    h.send(Msg::OpenActions).await;
    h.send(Msg::Escape).await;
    assert_eq!(h.model.view.mode, Mode::List);
    assert!(h.window_visible());
}

#[tokio::test]
async fn action_list_lists_both_totp_fields() {
    let mut h = opened("github").await;
    h.send(Msg::OpenActions).await;
    let labels: Vec<_> = h.model.actions().into_iter().map(|a| a.label).collect();
    assert!(labels.contains(&"One-time code".to_owned()));
    assert!(labels.contains(&"Backup".to_owned()));
    let backup = labels.iter().position(|l| l == "Backup").unwrap();
    h.send(Msg::ActivateAction(Some(backup))).await;
    assert_eq!(last_copy(&h), Some(("222222".into(), true)));
}

#[tokio::test]
async fn action_list_lists_every_website() {
    let mut h = opened("github").await;
    h.send(Msg::OpenActions).await;
    let labels: Vec<_> = h.model.actions().into_iter().map(|a| a.label).collect();
    assert!(labels.contains(&"Website".to_owned()));
    let second = labels.iter().position(|l| l == "Website 2").unwrap();
    h.send(Msg::ActivateAction(Some(second))).await;
    assert_eq!(
        last_copy(&h),
        Some(("https://gist.github.com".into(), false))
    );
    assert_eq!(
        (h.backend.field_calls(), h.backend.totp_calls()),
        (1, 0),
        "a website is fetched on copy, not taken from the summary"
    );
}

#[tokio::test]
async fn copy_url_shortcut_uses_the_first_website() {
    let mut h = opened("github").await;
    h.send(Msg::OpenActions).await;
    let with_shortcut: Vec<_> = h
        .model
        .actions()
        .into_iter()
        .filter(|a| a.shortcut == Some(cosmic_pass::config::Action::CopyUrl))
        .map(|a| a.label)
        .collect();
    assert_eq!(with_shortcut, vec!["Website".to_owned()]);
    h.send(Msg::CopyUrl).await;
    assert_eq!(
        last_copy(&h),
        Some(("https://github.com/login".into(), false))
    );
}

#[tokio::test]
async fn action_list_selection_is_clamped() {
    let mut h = opened("visa").await;
    h.send(Msg::OpenActions).await;
    for _ in 0..10 {
        h.send(Msg::SelectNext).await;
    }
    assert!(matches!(
        h.model.view.mode,
        Mode::Actions { selected: 2, .. }
    ));
    h.send(Msg::SelectPrev).await;
    assert!(matches!(
        h.model.view.mode,
        Mode::Actions { selected: 1, .. }
    ));
}

#[tokio::test]
async fn open_actions_without_selection_does_nothing() {
    let mut h = opened("zzzz").await;
    h.send(Msg::OpenActions).await;
    assert_eq!(h.model.view.mode, Mode::List);
}

#[tokio::test]
async fn a_user_defined_text_field_is_neither_listed_nor_copyable() {
    let mut h = opened("nickname").await;
    h.send(Msg::OpenActions).await;
    let labels: Vec<_> = h.model.actions().into_iter().map(|a| a.label).collect();
    assert_eq!(
        labels,
        ["Password", "Username", "Website", "One-time code"],
        "the text field has no value to show and no way to be copied, so it has no row"
    );
    // Nothing behind the list can reach it either: it never entered the item.
    let item = h
        .model
        .data
        .items
        .iter()
        .find(|i| i.key == key("nick"))
        .expect("the item is listed");
    assert!(item.field("Nickname").is_none());
}

#[tokio::test]
async fn dropping_a_text_field_leaves_the_dedicated_shortcuts_where_they_were() {
    let mut h = opened("nickname").await;
    h.send(Msg::CopyUsername).await;
    assert_eq!(last_copy(&h), Some(("ada".into(), false)));

    let mut h = opened("nickname").await;
    h.send(Msg::CopyUrl).await;
    assert_eq!(last_copy(&h), Some(("https://nick.example".into(), false)));

    let mut h = opened("nickname").await;
    h.send(Msg::CopyTotp).await;
    assert_eq!(last_copy(&h), Some(("654321".into(), true)));
}
