//! Action list for the selected item.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, icon, row, scrollable, space, text};

use crate::app::Message;
use crate::app::surface::WIDTH;
use crate::config::Action;
use crate::core::actions::CopySource;
use crate::core::state::{Model, Msg};

/// Stands in for a value line with nothing to show, as the detail pane does.
const NO_VALUE: &str = "—";

/// The value line under an action's label. A non-secret field can still arrive without a
/// value: custom fields of variant `Text` are cached by name only, so the action list has
/// nothing to preview and an empty line would read as an empty field.
fn value_hint(source: &CopySource) -> String {
    match source {
        CopySource::Field(f) if f.secret => "••••••••".to_owned(),
        CopySource::Field(f) => f.value.clone().unwrap_or_else(|| NO_VALUE.to_owned()),
        CopySource::Totp { .. } => "••••••".to_owned(),
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

pub fn view(model: &Model, selected: usize) -> Element<'_, Message> {
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
        .push(text::title4(item.display_title()))
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
    let rows = entries.into_iter().enumerate().map(|(i, entry)| {
        let hint = value_hint(&entry.source);
        let own = entry
            .shortcut
            .filter(|a| *a != Action::CopyPrimary)
            .map(|a| model.prefs.chord(a).to_string());
        let shortcut = shortcut_caption(own, (i == selected).then(|| activation.clone()));
        let line = row::with_capacity(4)
            .spacing(12)
            .align_y(Alignment::Center)
            .push(
                column::with_capacity(2)
                    .push(text::body(entry.label))
                    .push(text::caption(hint)),
            )
            .push(space::horizontal().width(Length::Fill))
            .push(text::caption(shortcut));
        let row = button::custom(line)
            .class(cosmic::theme::Button::ListItem(super::list::row_radii()))
            .selected(i == selected)
            .width(Length::Fill)
            .padding([6, 12])
            .on_press(Message::Core(Msg::ActivateAction(Some(i))));
        super::list::highlighted(row, i == selected)
    });

    let mut content = column::with_capacity(3).spacing(8).push(header);
    for line in super::notices(model) {
        content = content.push(text::caption(line));
    }
    content =
        content.push(scrollable(column::with_children(rows).spacing(2)).height(Length::Shrink));
    container(content).width(Length::Fixed(WIDTH)).into()
}

#[cfg(test)]
mod tests {
    use super::{shortcut_caption, value_hint};
    use crate::core::actions::CopySource;
    use crate::model::FieldRef;

    #[test]
    fn a_field_cached_without_its_value_shows_a_placeholder() {
        let source = CopySource::Field(FieldRef::unstored("API key", "API key"));
        assert_eq!(value_hint(&source), "—");
    }

    #[test]
    fn a_stored_value_is_shown_and_a_secret_stays_masked() {
        let stored = CopySource::Field(FieldRef::plain("username", "Username", "ada".into()));
        assert_eq!(value_hint(&stored), "ada");
        let secret = CopySource::Field(FieldRef::secret("password", "Password"));
        assert_eq!(value_hint(&secret), "••••••••");
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
