//! Search field and result list.

use cosmic::Element;
use cosmic::font::Font;
use cosmic::iced::advanced::text::{LineHeight, Span};
use cosmic::iced::widget::rich_text;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input};

use crate::app::Message;
use crate::app::surface::{SEARCH_INPUT, WIDTH};
use crate::core::state::{Model, Msg};
use crate::model::{ItemKind, ItemSummary};

/// The body typography preset, repeated here because a highlighted title is built from
/// spans and so cannot use `text::body`.
const BODY_SIZE: f32 = 14.0;
const BODY_LINE_HEIGHT: f32 = 21.0;

/// Corner radii for a selectable row.
pub fn row_radii() -> [f32; 4] {
    cosmic::theme::active().cosmic().corner_radii.radius_s
}

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

/// Splits `title` into runs of characters that did and did not match the query.
/// `indices` are character positions into `title`, sorted and unique.
fn title_runs<'a>(title: &'a str, indices: &[u32]) -> Vec<(&'a str, bool)> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut matched = false;
    let mut next = 0;
    for (position, (offset, _)) in title.char_indices().enumerate() {
        while indices.get(next).is_some_and(|i| (*i as usize) < position) {
            next += 1;
        }
        let here = indices.get(next).is_some_and(|i| *i as usize == position);
        if offset > start && here != matched {
            runs.push((&title[start..offset], matched));
            start = offset;
        }
        matched = here;
    }
    if start < title.len() {
        runs.push((&title[start..], matched));
    }
    runs
}

/// The item title, with the characters that matched the query in bold.
fn title<'a>(item: &'a ItemSummary, indices: &[u32]) -> Element<'a, Message> {
    let shown = item.display_title();
    // The indices point into the raw title; a blank title is shown as a placeholder instead.
    if indices.is_empty() || shown != item.title {
        return text::body(shown).into();
    }
    let spans: Vec<Span<'_, (), Font>> = title_runs(shown, indices)
        .into_iter()
        .map(|(part, matched)| {
            let span = Span::new(part);
            if matched {
                span.font(cosmic::font::bold())
            } else {
                span
            }
        })
        .collect();
    rich_text(spans)
        .size(BODY_SIZE)
        .line_height(LineHeight::Absolute(BODY_LINE_HEIGHT.into()))
        .font(cosmic::font::default())
        .into()
}

pub fn search_field(model: &Model) -> Element<'_, Message> {
    let input = text_input::search_input("Search Proton Pass", &model.view.query)
        .on_input(|q| Message::Core(Msg::QueryChanged(q)))
        .on_paste(|q| Message::Core(Msg::QueryChanged(q)))
        .on_submit(|_| Message::Submit)
        .id(SEARCH_INPUT.clone())
        // The popup owns the keyboard while it is open, so the field is always the target;
        // an explicit focus task can race with layer-surface creation and be lost.
        .always_active()
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

    if let Some(status) = model.stale_notice() {
        content = content.push(text::caption(status));
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
            .filter_map(|(i, hit)| model.data.items.get(hit.item).map(|item| (i, hit, item)))
            .map(|(i, hit, item)| {
                let mut labels = column::with_capacity(2).push(title(item, &hit.title_indices));
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
                    // ListItem paints a selected state; MenuItem does not.
                    .class(cosmic::theme::Button::ListItem(row_radii()))
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

#[cfg(test)]
mod tests {
    use super::title_runs;

    #[test]
    fn unmatched_title_is_one_run() {
        assert_eq!(title_runs("GitHub", &[]), [("GitHub", false)]);
        assert_eq!(title_runs("", &[]), []);
    }

    #[test]
    fn matched_characters_form_their_own_runs() {
        assert_eq!(
            title_runs("GitHub", &[0, 2, 3]),
            [("G", true), ("i", false), ("tH", true), ("ub", false)]
        );
        assert_eq!(
            title_runs("GitHub", &[4, 5]),
            [("GitH", false), ("ub", true)]
        );
    }

    #[test]
    fn indices_are_character_positions_not_bytes() {
        // "é" and "ü" are two bytes each; slicing by byte offset would panic or split them.
        assert_eq!(
            title_runs("céü1", &[1, 2]),
            [("c", false), ("éü", true), ("1", false)]
        );
    }

    #[test]
    fn indices_past_the_end_are_ignored() {
        assert_eq!(title_runs("ab", &[1, 9]), [("a", false), ("b", true)]);
    }
}
