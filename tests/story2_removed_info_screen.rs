//! Acceptance tests for User Story 2 of feature 002: one screen for an item's fields.
//!
//! The field list behind Tab is now the only per-item screen. These tests drive the four
//! acceptance scenarios end to end: the chord that used to open the info screen opens
//! nothing, the preferences editor offers no binding for it, a preference file written
//! before the removal still loads with every other chord intact, and everything the removed
//! screen displayed is still reachable from the field list.
//!
//! The editor and the field list are asserted through the model they render rather than
//! through their widgets: `preferences::view` walks `Action::ALL` and `actions::view` walks
//! `Model::actions`, and the libcosmic view glue is excluded from the coverage gates.

use cosmic_pass::app::keys::{KeyContext, KeyPress, map_key};
use cosmic_pass::config::{Action, KeyChord, Modifier, Preferences, Shortcuts};
use cosmic_pass::core::state::{Mode, Msg};
use cosmic_pass::model::{FieldRef, ItemKey, ItemKind, ItemSummary};
use cosmic_pass::testing::{FakeBackend, Harness, summary};

/// The chord the removed info screen was bound to.
const FORMER_INFO_CHORD: (&[Modifier], &str) = (&[Modifier::Ctrl], "i");

fn key(id: &str) -> ItemKey {
    ItemKey::new("s", id)
}

fn github() -> ItemSummary {
    let mut i = summary("gh", "GitHub", ItemKind::Login);
    i.username = Some("octocat".into());
    i.vault_name = "Work".into();
    i.fields = vec![
        FieldRef::plain("username", "Username", "octocat".into()),
        FieldRef::secret("password", "Password"),
        FieldRef::plain("url", "Website", "https://github.com".into()),
    ];
    i.totp_fields = vec!["totp_uri".into()];
    i
}

async fn opened(prefs: Preferences) -> Harness {
    let backend = FakeBackend::with_items(vec![github()]);
    backend.set_field(&key("gh"), "password", "SECRET-FIXTURE-gh");
    backend.set_totp(&key("gh"), &[("totp_uri", "123456")]);
    let mut h = Harness::with_prefs(backend, prefs);
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::QueryChanged("github".into())).await;
    h
}

fn press(h: &Harness, modifiers: &[Modifier], key: &str) -> Option<Msg> {
    map_key(
        &KeyPress::new(modifiers, key),
        &h.model.view.mode,
        &h.model.prefs,
        KeyContext::default(),
    )
}

/// Scenario 1: an item is highlighted and the user presses the chord that used to open the
/// info screen. Nothing opens, and the press is not swallowed into some other action.
#[tokio::test]
async fn the_former_info_chord_opens_nothing() {
    let mut h = opened(Preferences::default()).await;
    let (modifiers, code) = FORMER_INFO_CHORD;

    assert!(
        press(&h, modifiers, code).is_none(),
        "no message is mapped from the result list"
    );
    assert_eq!(h.model.view.mode, Mode::List, "so the mode cannot change");

    h.send(Msg::OpenActions).await;
    assert!(
        matches!(h.model.view.mode, Mode::Actions { .. }),
        "the field list is the screen that does open"
    );
    assert!(
        press(&h, modifiers, code).is_none(),
        "and the chord is dead inside it too"
    );
}

/// Scenario 2: the preferences editor offers no binding for the removed action. It renders
/// one row per `Action::ALL` entry, so the absence is asserted there.
#[tokio::test]
async fn the_editor_offers_no_binding_for_the_removed_action() {
    let mut h = opened(Preferences::default()).await;
    h.send(Msg::OpenPreferences).await;
    assert!(matches!(h.model.view.mode, Mode::Preferences { .. }));

    assert_eq!(
        Action::ALL.len(),
        10,
        "the detail action is gone from the rows"
    );
    for action in Action::ALL {
        assert!(
            !action.label().to_lowercase().contains("detail"),
            "{action:?} still offers a detail screen"
        );
        assert_ne!(
            h.model.prefs.chord(action),
            KeyChord::new(FORMER_INFO_CHORD.0, FORMER_INFO_CHORD.1),
            "{action:?} inherited the removed action's chord"
        );
    }
}

/// Scenario 3: a preference file written before the removal — in the RON shape cosmic-config
/// stores, still naming `open_detail`, with one chord the user had rebound — loads, and the
/// rebound chord goes on working end to end.
#[tokio::test]
async fn a_preference_file_naming_the_removed_action_keeps_its_other_chords() {
    let stored = r#"{
        open_detail: (modifiers: [Ctrl], key: "i"),
        copy_username: (modifiers: [Ctrl], key: "y"),
    }"#;
    let shortcuts: Shortcuts = ron::from_str(stored).expect("a stored file must still load");
    let prefs = Preferences {
        shortcuts,
        ..Preferences::default()
    }
    .validated();

    let mut h = opened(prefs).await;
    let msg = press(&h, &[Modifier::Ctrl], "y").expect("the rebound chord still copies");
    h.send(msg).await;

    let copied: Vec<(String, bool)> = h
        .clipboard
        .copies()
        .into_iter()
        .map(|(value, secret, _)| (value, secret))
        .collect();
    assert_eq!(
        copied,
        vec![("octocat".to_owned(), false)],
        "the username the user rebound Ctrl+Y to reaches the clipboard"
    );
    assert!(
        press(&h, FORMER_INFO_CHORD.0, FORMER_INFO_CHORD.1).is_none(),
        "while the stale binding does nothing"
    );
}

/// Scenario 4: everything the removed screen showed is still reachable from the field list —
/// the title, the kind, the vault, every field, and the one-time code.
#[tokio::test]
async fn the_field_list_still_reaches_everything_the_screen_showed() {
    let mut h = opened(Preferences::default()).await;
    h.send(Msg::OpenActions).await;

    let item = h.model.target_item().expect("the field list has its item");
    assert_eq!(item.display_title(), "GitHub");
    assert_eq!(item.kind, ItemKind::Login);
    assert_eq!(item.vault_name, "Work");

    let labels: Vec<String> = h.model.actions().into_iter().map(|e| e.label).collect();
    for field in &item.fields {
        assert!(
            labels.contains(&field.label),
            "{} is not listed: {labels:?}",
            field.label
        );
    }
    assert!(
        labels.contains(&"One-time code".to_owned()),
        "the code is not listed: {labels:?}"
    );
}
