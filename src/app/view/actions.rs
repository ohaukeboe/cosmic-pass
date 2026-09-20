//! Action list for the selected item.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, icon, row, scrollable, space, text};
use secrecy::ExposeSecret;

use crate::app::Message;
use crate::app::surface::WIDTH;
use crate::config::Action;
use crate::core::actions::{ActionEntry, CopySource};
use crate::core::state::{Model, Msg};
use crate::model::ItemKind;

/// Stands in for a value line with nothing to show. Now that user-defined text fields are
/// dropped at parse time, the only rows left that reach it are the built-in members fetched
/// on demand (`phone_number`, `first_name`, `city`, ...), which are cached by name only, so
/// the list holds nothing to preview until one is copied (FR-115).
const NO_VALUE: &str = "—";
/// A secret field's masked value line.
const MASK: &str = "••••••••";
/// A one-time code's masked value line, as wide as the code it hides.
const TOTP_MASK: &str = "••••••";

/// What kind of item this is, for the line under its title. This is the only per-item
/// screen, so it is the only place the kind is named.
fn kind_label(kind: &ItemKind) -> &str {
    match kind {
        ItemKind::Login => "Login",
        ItemKind::Note => "Note",
        ItemKind::CreditCard => "Credit card",
        ItemKind::Identity => "Identity",
        ItemKind::Alias => "Alias",
        ItemKind::SshKey => "SSH key",
        ItemKind::Wifi => "Wi-Fi",
        ItemKind::Custom => "Custom",
        ItemKind::Unknown(name) => name,
    }
}

/// What the reveal chord would do to the highlighted row, and so what the caption names it as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reveal {
    /// A masked value the chord would uncover.
    Masked,
    /// The value this list currently shows; the same chord masks it again.
    Shown,
    /// A value already on its own line: the chord does nothing here (FR-105).
    Plain,
}

/// Which row of the list, if any, the value on screen belongs to.
fn reveal_state(model: &Model, source: &CopySource, field_index: Option<usize>) -> Reveal {
    match source {
        CopySource::Totp { field } => {
            if model.revealed_totp().is_some_and(|t| t.field == *field) {
                Reveal::Shown
            } else {
                Reveal::Masked
            }
        }
        CopySource::Field(f) if f.secret => {
            if field_index.is_some_and(|index| model.revealed_value(index).is_some()) {
                Reveal::Shown
            } else {
                Reveal::Masked
            }
        }
        CopySource::Field(_) => Reveal::Plain,
    }
}

/// The value line under an action's label. A secret stays masked unless it is the row the
/// list reveals, and a revealed one-time code carries the seconds left in its period
/// (FR-104). A non-secret field can still arrive without a value: built-in members fetched on
/// demand are cached by name only, so the list has nothing to preview and an empty line would
/// read as an empty field.
fn value_hint(model: &Model, source: &CopySource, field_index: Option<usize>, now: i64) -> String {
    match source {
        CopySource::Totp { field } => match model.revealed_totp() {
            Some(shown) if shown.field == *field => {
                format!("{} · {}s", shown.code.expose_secret(), shown.remaining(now))
            }
            _ => TOTP_MASK.to_owned(),
        },
        CopySource::Field(f) if f.secret => field_index
            .and_then(|index| model.revealed_value(index))
            .map_or_else(|| MASK.to_owned(), |v| v.expose_secret().to_owned()),
        CopySource::Field(f) => f.value.clone().unwrap_or_else(|| NO_VALUE.to_owned()),
    }
}

/// Whether the value for this row is still being fetched, so the row can say so rather than
/// sitting masked as if the press had been ignored.
fn fetching(model: &Model, source: &CopySource, field_index: Option<usize>) -> bool {
    match source {
        CopySource::Totp { .. } => model.revealing_totp(),
        CopySource::Field(_) => field_index.is_some_and(|index| model.revealing_field(index)),
    }
}

/// The chord caption for a row: its own chord, if any, plus the activation chord when the
/// row is highlighted. `Enter` copies the highlighted field rather than the primary one
/// (keyboard.md, action list mode), so its caption has to follow the highlight.
fn shortcut_caption(own: Option<String>, activation: Option<String>) -> String {
    match (activation, own) {
        (Some(a), Some(o)) => format!("{a} · {o}"),
        (Some(c), None) | (None, Some(c)) => c,
        (None, None) => String::new(),
    }
}

/// What the list says instead of showing nothing. An item can be left with no listable field
/// at all — a note whose only custom field was a text one, say — and a blank scroll area
/// would read as a screen that failed to load (FR-117).
fn empty_notice(entries: &[ActionEntry]) -> Option<&'static str> {
    entries
        .is_empty()
        .then_some("This item has no copyable fields")
}

/// The footer line naming what the chords do to the highlighted row (FR-107). A row whose
/// value is already printed is not worth offering a reveal for.
fn reveal_caption(state: Option<Reveal>, reveal: &str, copy: &str) -> String {
    match state {
        Some(Reveal::Masked) => format!("{reveal} to reveal · {copy} to copy"),
        Some(Reveal::Shown) => format!("{reveal} to hide · {copy} to copy"),
        Some(Reveal::Plain) | None => format!("{copy} to copy"),
    }
}

pub fn view(model: &Model, selected: usize, now: i64) -> Element<'_, Message> {
    let Some(item) = model.target_item() else {
        return text::body("Item no longer exists").into();
    };
    let mut header = row::with_capacity(6)
        .spacing(12)
        .align_y(Alignment::Center)
        .push(
            button::icon(icon::from_name("go-previous-symbolic"))
                .on_press(Message::Core(Msg::Back)),
        )
        .push(icon::from_name(super::list::icon_name(&item.kind)).size(20))
        .push(
            column::with_capacity(2)
                .push(text::title4(item.display_title()))
                .push(text::caption(kind_label(&item.kind))),
        )
        .push(space::horizontal().width(Length::Fill));
    // A copy started from here keeps this mode, so the fetch would otherwise be invisible.
    if model
        .view
        .pending
        .as_ref()
        .is_some_and(|p| p.key == item.key)
    {
        header = header.push(icon::from_name("view-refresh-symbolic").size(16));
    }
    let header = header.push(text::caption(item.vault_name.as_str()));

    let activation = model.prefs.chord(Action::CopyPrimary).to_string();
    let entries = model.actions();
    let highlighted = entries
        .get(selected)
        .map(|e| reveal_state(model, &e.source, e.field_index));
    let empty = empty_notice(&entries);
    let rows = entries.into_iter().enumerate().map(|(i, entry)| {
        let hint = value_hint(model, &entry.source, entry.field_index, now);
        let own = entry
            .shortcut
            .filter(|a| *a != Action::CopyPrimary)
            .map(|a| model.prefs.chord(a).to_string());
        let shortcut = shortcut_caption(own, (i == selected).then(|| activation.clone()));
        let mut line = row::with_capacity(4)
            .spacing(12)
            .align_y(Alignment::Center)
            .push(
                column::with_capacity(2)
                    .push(text::body(entry.label))
                    .push(text::caption(hint)),
            )
            .push(space::horizontal().width(Length::Fill));
        if fetching(model, &entry.source, entry.field_index) {
            line = line.push(icon::from_name("view-refresh-symbolic").size(16));
        }
        let line = line.push(text::caption(shortcut));
        let row = button::custom(line)
            .class(cosmic::theme::Button::ListItem(super::list::row_radii()))
            .selected(i == selected)
            .width(Length::Fill)
            .padding([6, 12])
            .on_press(Message::Core(Msg::ActivateAction(Some(i))));
        super::list::highlighted(row, i == selected)
    });

    let mut content = column::with_capacity(4).spacing(8).push(header);
    for line in super::notices(model) {
        content = content.push(text::caption(line));
    }
    // With nothing to list, the chord caption would name chords that do nothing to a row
    // that is not there, so the message stands in for both the rows and the footer.
    let content = match empty {
        Some(line) => content.push(text::caption(line)),
        None => content
            .push(scrollable(column::with_children(rows).spacing(2)).height(Length::Shrink))
            .push(text::caption(reveal_caption(
                highlighted,
                &model.prefs.chord(Action::Reveal).to_string(),
                &activation,
            ))),
    };
    container(content).width(Length::Fixed(WIDTH)).into()
}

#[cfg(test)]
mod tests {
    use super::{
        Reveal, empty_notice, fetching, kind_label, reveal_caption, reveal_state, shortcut_caption,
        value_hint,
    };
    use crate::core::actions::CopySource;
    use crate::core::effects::Effect;
    use crate::core::state::tests::loaded;
    use crate::core::state::{Model, Msg};
    use crate::model::{FieldRef, ItemKey, ItemKind, ItemSummary};
    use secrecy::SecretString;

    /// A card with a plain field, two secrets and a one-time code, shown as a field list.
    fn card() -> ItemSummary {
        ItemSummary {
            kind: ItemKind::CreditCard,
            fields: vec![
                FieldRef::plain("cardholder", "Cardholder", "Ada Lovelace".into()),
                FieldRef::secret("number", "Card number"),
                FieldRef::secret("cvv", "Verification number"),
            ],
            totp_fields: vec!["totp_uri".into()],
            ..crate::core::state::tests::login("a", "Visa")
        }
    }

    /// The field list open on the card, highlight moved `steps` rows down.
    fn list(steps: usize) -> Model {
        let mut m = loaded(vec![card()]);
        m.update(Msg::Show, 0);
        m.update(Msg::OpenActions, 0);
        for _ in 0..steps {
            m.update(Msg::SelectNext, 0);
        }
        m
    }

    /// Asks to reveal the highlighted row, returning the fetch's `(generation, index)`.
    fn start_reveal(m: &mut Model) -> (u64, usize) {
        match m.update(Msg::ToggleReveal, 0).first() {
            Some(Effect::FetchReveal {
                generation, index, ..
            }) => (*generation, *index),
            other => panic!("expected a reveal fetch, got {other:?}"),
        }
    }

    /// Delivers `value` for a fetch `start_reveal` began.
    fn deliver(m: &mut Model, (generation, index): (u64, usize), value: &str) {
        m.update(
            Msg::RevealFetched {
                key: ItemKey::new("s", "a"),
                index,
                generation,
                result: Ok(SecretString::from(value)),
            },
            0,
        );
    }

    /// Reveals the highlighted row and delivers `value` for it.
    fn reveal(m: &mut Model, value: &str) {
        let fetch = start_reveal(m);
        deliver(m, fetch, value);
    }

    /// Reveals the highlighted one-time-code row and delivers a code for it.
    fn reveal_code(m: &mut Model, code: &str, now: i64) {
        m.update(Msg::ToggleReveal, 0);
        m.update(
            Msg::TotpFetched {
                key: ItemKey::new("s", "a"),
                result: Ok([("totp_uri".to_owned(), SecretString::from(code))]
                    .into_iter()
                    .collect()),
            },
            now,
        );
    }

    fn hint(m: &Model, row: usize) -> String {
        let entry = m.actions().into_iter().nth(row).expect("row exists");
        value_hint(m, &entry.source, entry.field_index, 0)
    }

    #[test]
    fn the_header_names_the_kind_of_item_the_list_belongs_to() {
        assert_eq!(kind_label(&ItemKind::CreditCard), "Credit card");
        assert_eq!(kind_label(&ItemKind::SshKey), "SSH key");
        assert_eq!(
            kind_label(&ItemKind::Unknown("Membership".into())),
            "Membership",
            "a kind this version has no name for is shown as Proton Pass names it"
        );
    }

    #[test]
    fn a_field_cached_without_its_value_shows_a_placeholder() {
        let m = list(0);
        let source = CopySource::Field(FieldRef::unstored("API key", "API key"));
        assert_eq!(value_hint(&m, &source, None, 0), "—");
    }

    #[test]
    fn a_stored_value_is_shown_and_a_secret_stays_masked() {
        let m = list(0);
        // Row 1 is the cardholder, whose value the list already holds; rows 0 and 2 are secret.
        assert_eq!(hint(&m, 1), "Ada Lovelace");
        assert_eq!(hint(&m, 0), "••••••••");
        assert_eq!(hint(&m, 2), "••••••••");
    }

    #[test]
    fn only_the_revealed_row_shows_its_plaintext() {
        let mut m = list(0);
        reveal(&mut m, "4111 1111 1111 1111");
        assert_eq!(hint(&m, 0), "4111 1111 1111 1111");
        assert_eq!(hint(&m, 2), "••••••••", "the other secret stays masked");
        assert_eq!(hint(&m, 3), "••••••", "and so does the one-time code");
    }

    #[test]
    fn a_revealed_one_time_code_shows_the_seconds_left_in_its_period() {
        let mut m = list(3);
        assert_eq!(hint(&m, 3), "••••••");
        reveal_code(&mut m, "123456", 10);
        let entry = m.actions().into_iter().nth(3).expect("the code row");
        assert_eq!(
            value_hint(&m, &entry.source, entry.field_index, 10),
            "123456 · 20s"
        );
        assert_eq!(
            value_hint(&m, &entry.source, entry.field_index, 25),
            "123456 · 5s"
        );
        assert_eq!(hint(&m, 0), "••••••••", "the secrets stay masked");
    }

    /// The same card with a backup authenticator, so two rows can each hold a code.
    fn twin_code_card() -> ItemSummary {
        ItemSummary {
            totp_fields: vec!["totp_uri".into(), "backup".into()],
            ..card()
        }
    }

    #[test]
    fn only_the_revealed_code_row_shows_a_code() {
        let mut m = loaded(vec![twin_code_card()]);
        m.update(Msg::Show, 0);
        m.update(Msg::OpenActions, 0);
        // Row 4 is the backup code; row 3 is the card's first one.
        for _ in 0..4 {
            m.update(Msg::SelectNext, 0);
        }
        m.update(Msg::ToggleReveal, 0);
        // The fetch answers with both codes at once, as `pass-cli` does.
        m.update(
            Msg::TotpFetched {
                key: ItemKey::new("s", "a"),
                result: Ok([
                    ("totp_uri".to_owned(), SecretString::from("111111")),
                    ("backup".to_owned(), SecretString::from("222222")),
                ]
                .into_iter()
                .collect()),
            },
            0,
        );
        let entry = m.actions().into_iter().nth(4).expect("the backup code row");
        assert_eq!(
            value_hint(&m, &entry.source, entry.field_index, 0),
            "222222 · 30s"
        );
        assert_eq!(hint(&m, 3), "••••••", "the other code stays masked");
    }

    #[test]
    fn a_row_says_when_its_value_is_still_on_its_way() {
        let mut m = list(0);
        let entry = m.actions().into_iter().next().expect("the primary row");
        assert!(!fetching(&m, &entry.source, entry.field_index));
        let fetch = start_reveal(&mut m);
        assert!(fetching(&m, &entry.source, entry.field_index));
        let other = m.actions().into_iter().nth(2).expect("the other secret");
        assert!(
            !fetching(&m, &other.source, other.field_index),
            "only the row that was asked for"
        );
        deliver(&mut m, fetch, "4111");
        assert!(!fetching(&m, &entry.source, entry.field_index));
    }

    #[test]
    fn the_caption_names_what_the_reveal_chord_would_do_to_the_highlighted_row() {
        let masked = list(0);
        let entry = masked
            .actions()
            .into_iter()
            .next()
            .expect("the primary row");
        assert_eq!(
            reveal_state(&masked, &entry.source, entry.field_index),
            Reveal::Masked
        );
        let mut shown = list(0);
        reveal(&mut shown, "4111");
        let entry = shown.actions().into_iter().next().expect("the primary row");
        assert_eq!(
            reveal_state(&shown, &entry.source, entry.field_index),
            Reveal::Shown
        );
        let plain = list(1);
        let entry = plain.actions().into_iter().nth(1).expect("the plain row");
        assert_eq!(
            reveal_state(&plain, &entry.source, entry.field_index),
            Reveal::Plain
        );
        assert_eq!(
            reveal_caption(Some(Reveal::Masked), "Ctrl+R", "Enter"),
            "Ctrl+R to reveal · Enter to copy"
        );
        assert_eq!(
            reveal_caption(Some(Reveal::Shown), "Ctrl+R", "Enter"),
            "Ctrl+R to hide · Enter to copy"
        );
        assert_eq!(
            reveal_caption(Some(Reveal::Plain), "Ctrl+R", "Enter"),
            "Enter to copy"
        );
        assert_eq!(reveal_caption(None, "Ctrl+R", "Enter"), "Enter to copy");
    }

    /// An item can be left with nothing to list — a note whose only custom field was a text
    /// one, say — and a blank scroll area would read as a screen that failed to load.
    #[test]
    fn an_item_with_nothing_to_copy_says_so_instead_of_listing_nothing() {
        let bare = crate::core::state::tests::login("b", "Bare");
        let mut m = loaded(vec![bare]);
        m.update(Msg::Show, 0);
        m.update(Msg::OpenActions, 0);
        assert!(m.actions().is_empty(), "the fixture offers nothing to copy");
        assert_eq!(
            empty_notice(&m.actions()),
            Some("This item has no copyable fields")
        );
        assert_eq!(
            empty_notice(&list(0).actions()),
            None,
            "an item with rows says nothing extra"
        );
    }

    #[test]
    fn the_activation_chord_is_captioned_on_the_highlighted_row_only() {
        assert_eq!(shortcut_caption(None, Some("Enter".into())), "Enter");
        assert_eq!(shortcut_caption(None, None), "");
    }

    #[test]
    fn a_highlighted_row_keeps_its_own_chord_alongside_the_activation_chord() {
        assert_eq!(shortcut_caption(Some("Ctrl+U".into()), None), "Ctrl+U");
        assert_eq!(
            shortcut_caption(Some("Ctrl+U".into()), Some("Enter".into())),
            "Enter · Ctrl+U"
        );
    }
}
