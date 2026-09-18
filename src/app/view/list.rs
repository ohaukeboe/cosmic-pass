//! Search field and result list.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input};

use crate::app::Message;
use crate::app::surface::{SEARCH_INPUT, WIDTH};
use crate::core::state::{Model, Msg};
use crate::model::ItemKind;

pub fn icon_name(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::Login => "dialog-password-symbolic",
        ItemKind::Note => "x-office-document-symbolic",
        ItemKind::CreditCard => "payment-card-symbolic",
        ItemKind::Identity => "contact-new-symbolic",
        ItemKind::Alias => "mail-forward-symbolic",
        ItemKind::Wifi => "network-wireless-symbolic",
        ItemKind::SshKey => "utilities-terminal-symbolic",
        ItemKind::Custom | ItemKind::Unknown(_) => "emblem-documents-symbolic",
    }
}

pub fn search_field(model: &Model) -> Element<'_, Message> {
    let input = text_input::search_input("Search Proton Pass", &model.view.query)
        .on_input(|q| Message::Core(Msg::QueryChanged(q)))
        .on_paste(|q| Message::Core(Msg::QueryChanged(q)))
        .on_submit(|_| Message::Submit)
        .id(SEARCH_INPUT.clone())
        .width(Length::Fill);
    let mut header = row::with_capacity(2)
        .push(input)
        .spacing(8)
        .align_y(Alignment::Center);
    if model.data.refreshing {
        header = header.push(icon::from_name("view-refresh-symbolic").size(16));
    }
    header.into()
}

pub fn view(model: &Model) -> Element<'_, Message> {
    let mut content = column::with_capacity(3)
        .spacing(8)
        .push(search_field(model));

    if model.data.stale && !model.data.refreshing {
        content = content.push(text::caption(
            "Data may be out of date. Press F5 to refresh.",
        ));
    }

    if let Some(notice) = &model.view.notice {
        content = content.push(text::caption(notice.text.as_str()));
    }

    if model.view.results.is_empty() {
        let message = if model.data.items.is_empty() && model.data.refreshing {
            "Loading items…"
        } else {
            "No items found"
        };
        content = content.push(
            container(text::body(message))
                .padding(16)
                .width(Length::Fill)
                .align_x(Alignment::Center),
        );
    } else {
        let pending = model.view.pending.as_ref().map(|p| &p.key);
        let rows = model
            .view
            .results
            .iter()
            .enumerate()
            .filter_map(|(i, r)| model.data.items.get(r.item).map(|item| (i, item)))
            .map(|(i, item)| {
                let mut labels = column::with_capacity(2).push(text::body(item.display_title()));
                if let Some(sub) = &item.subtitle {
                    labels = labels.push(text::caption(sub.as_str()));
                }
                let mut line = row::with_capacity(4)
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .push(icon::from_name(icon_name(&item.kind)).size(20))
                    .push(labels)
                    .push(widget::space::horizontal().width(Length::Fill));
                if pending == Some(&item.key) {
                    line = line.push(icon::from_name("view-refresh-symbolic").size(16));
                }
                line = line.push(text::caption(item.vault_name.as_str()));
                button::custom(line)
                    .class(cosmic::theme::Button::MenuItem)
                    .selected(i == model.view.selected)
                    .width(Length::Fill)
                    .padding([6, 12])
                    .on_press(Message::RowPressed(i))
                    .into()
            });
        content = content.push(
            scrollable(column::with_children(rows).spacing(2))
                .id(super::RESULTS_ID.clone())
                .height(Length::Shrink),
        );
    }

    container(content).width(Length::Fixed(WIDTH)).into()
}
