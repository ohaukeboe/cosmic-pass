//! Preferences editor.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, divider, icon, row, scrollable, space, text};

use crate::app::Message;
use crate::app::surface::WIDTH;
use crate::config::{Action, CLIPBOARD_CLEAR_STEP};
use crate::core::state::{Mode, Model, Msg};

pub fn view(model: &Model) -> Element<'_, Message> {
    let rebinding = match model.view.mode {
        Mode::Preferences { rebinding } => rebinding,
        _ => None,
    };
    let prefs = &model.prefs;

    let header = row::with_capacity(3)
        .spacing(12)
        .align_y(Alignment::Center)
        .push(
            button::icon(icon::from_name("go-previous-symbolic"))
                .on_press(Message::Core(Msg::Back)),
        )
        .push(text::title4("Preferences"));

    let timeout = row::with_capacity(5)
        .spacing(8)
        .align_y(Alignment::Center)
        .push(text::body("Clear copied secrets after"))
        .push(space::horizontal().width(Length::Fill))
        .push(
            button::icon(icon::from_name("list-remove-symbolic")).on_press(Message::Core(
                Msg::AdjustClipboardClear(-CLIPBOARD_CLEAR_STEP),
            )),
        )
        .push(text::body(format!("{} s", prefs.clipboard_clear_secs)))
        .push(
            button::icon(icon::from_name("list-add-symbolic")).on_press(Message::Core(
                Msg::AdjustClipboardClear(CLIPBOARD_CLEAR_STEP),
            )),
        );

    let mut shortcuts = column::with_capacity(Action::ALL.len()).spacing(2);
    for action in Action::ALL {
        let label = if rebinding == Some(action) {
            "Press keys… (Esc to cancel)".to_owned()
        } else {
            prefs.chord(action).to_string()
        };
        shortcuts = shortcuts.push(
            row::with_capacity(3)
                .spacing(8)
                .align_y(Alignment::Center)
                .push(text::body(action.label()))
                .push(space::horizontal().width(Length::Fill))
                .push(button::standard(label).on_press(Message::Core(Msg::StartRebind(action)))),
        );
    }

    let mut content = column::with_capacity(8)
        .spacing(12)
        .push(header)
        .push(timeout)
        .push(divider::horizontal::default())
        .push(text::heading("Keyboard shortcuts"))
        .push(text::caption(
            "Left/Right adjust the timeout · press a shortcut to rebind it · Ctrl+Shift+Delete resets",
        ));
    // Only the notice, not the shared caption pair: the stale line points at F5, which the
    // editor rebinds rather than obeys, and nothing here shows item data anyway.
    if let Some(notice) = &model.view.notice {
        content = content.push(text::caption(notice.text.as_str()));
    }
    content = content
        .push(scrollable(shortcuts).height(Length::Shrink))
        .push(button::text("Reset shortcuts").on_press(Message::Core(Msg::ResetShortcuts)));

    container(content).width(Length::Fixed(WIDTH)).into()
}
