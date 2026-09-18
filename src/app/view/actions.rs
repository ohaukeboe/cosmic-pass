//! Action list for the selected item.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, icon, row, scrollable, space, text};

use crate::app::Message;
use crate::app::surface::WIDTH;
use crate::core::actions::CopySource;
use crate::core::state::{Model, Msg};

pub fn view(model: &Model, selected: usize) -> Element<'_, Message> {
    let Some(item) = model.target_item() else {
        return text::body("Item no longer exists").into();
    };
    let header = row::with_capacity(4)
        .spacing(12)
        .align_y(Alignment::Center)
        .push(
            button::icon(icon::from_name("go-previous-symbolic"))
                .on_press(Message::Core(Msg::Back)),
        )
        .push(icon::from_name(super::list::icon_name(&item.kind)).size(20))
        .push(text::title4(item.display_title()))
        .push(space::horizontal().width(Length::Fill))
        .push(text::caption(item.vault_name.as_str()));

    let entries = model.actions();
    let rows = entries.into_iter().enumerate().map(|(i, entry)| {
        let hint = match &entry.source {
            CopySource::Field(f) if f.secret => "••••••••".to_owned(),
            CopySource::Field(f) => f.value.clone().unwrap_or_default(),
            CopySource::Totp { .. } => "••••••".to_owned(),
        };
        let shortcut = entry
            .shortcut
            .map(|a| model.prefs.chord(a).to_string())
            .unwrap_or_default();
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
        button::custom(line)
            .class(cosmic::theme::Button::MenuItem)
            .selected(i == selected)
            .width(Length::Fill)
            .padding([6, 12])
            .on_press(Message::Core(Msg::ActivateAction(Some(i))))
            .into()
    });

    let content = column::with_capacity(2)
        .spacing(8)
        .push(header)
        .push(scrollable(column::with_children(rows).spacing(2)).height(Length::Shrink));
    container(content).width(Length::Fixed(WIDTH)).into()
}
