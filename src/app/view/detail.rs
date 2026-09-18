//! Detail pane with masked secrets and TOTP countdown.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, divider, icon, progress_bar, row, space, text};
use secrecy::ExposeSecret;

use crate::app::Message;
use crate::app::surface::WIDTH;
use crate::config::Action;
use crate::core::actions::{CopySource, primary_action};
use crate::core::state::{Model, Msg};
use crate::model::{FieldRef, ItemKind};

const MASK: &str = "••••••••";

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

/// The `(label, shown value)` pairs of the detail pane, in display order. Secret values are
/// masked unless revealed. Every website is already a field of its own, so `urls` is not
/// rendered separately.
pub fn rows(model: &Model, now: i64) -> Vec<(String, String)> {
    let _ = now;
    let Some(item) = model.target_item() else {
        return Vec::new();
    };
    let primary = match primary_action(item) {
        Some(CopySource::Field(f)) if f.secret => Some(f.name),
        _ => None,
    };
    item.fields
        .iter()
        .map(
            |FieldRef {
                 name,
                 label,
                 value,
                 secret,
             }| {
                let shown = match (value, *secret) {
                    (Some(v), false) => v.clone(),
                    (_, true) if primary.as_deref() == Some(name.as_str()) => model
                        .view
                        .revealed
                        .as_ref()
                        .map_or_else(|| MASK.to_owned(), |v| v.expose_secret().to_owned()),
                    (_, true) => MASK.to_owned(),
                    (None, false) => "—".to_owned(),
                };
                (label.clone(), shown)
            },
        )
        .collect()
}

fn field_row_owned<'a>(label: String, value: String) -> Element<'a, Message> {
    row::with_capacity(3)
        .spacing(12)
        .push(text::caption(label).width(Length::Fixed(140.0)))
        .push(text::body(value))
        .into()
}

fn field_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    row::with_capacity(3)
        .spacing(12)
        .push(text::caption(label).width(Length::Fixed(140.0)))
        .push(text::body(value))
        .into()
}

pub fn view(model: &Model, now: i64) -> Element<'_, Message> {
    let Some(item) = model.target_item() else {
        return text::body("Item no longer exists").into();
    };
    let header = row::with_capacity(5)
        .spacing(12)
        .align_y(Alignment::Center)
        .push(
            button::icon(icon::from_name("go-previous-symbolic"))
                .on_press(Message::Core(Msg::Back)),
        )
        .push(icon::from_name(super::list::icon_name(&item.kind)).size(24))
        .push(
            column::with_capacity(2)
                .push(text::title4(item.display_title()))
                .push(text::caption(format!(
                    "{} · {}",
                    kind_label(&item.kind),
                    item.vault_name
                ))),
        )
        .push(space::horizontal().width(Length::Fill));

    let mut body = column::with_capacity(item.fields.len() + 4).spacing(8);
    for (label, shown) in rows(model, now) {
        body = body.push(field_row_owned(label, shown));
    }

    if let Some(totp) = &model.view.totp {
        let remaining = totp.remaining(now);
        #[allow(clippy::cast_precision_loss)]
        let bar = progress_bar::determinate_linear(remaining as f32 / totp.period.max(1) as f32);
        body = body
            .push(divider::horizontal::light())
            .push(
                row::with_capacity(3)
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .push(text::caption("One-time code").width(Length::Fixed(140.0)))
                    .push(text::title3(totp.code.expose_secret().to_owned()))
                    .push(text::caption(format!("{remaining}s"))),
            )
            .push(bar);
    } else if item.has_totp() {
        body = body.push(field_row("One-time code", "…".to_owned()));
    }

    let has_secret = matches!(primary_action(item), Some(CopySource::Field(f)) if f.secret);
    let reveal_hint = if has_secret {
        format!(
            "{} to reveal · {} to copy",
            model.prefs.chord(Action::Reveal),
            model.prefs.chord(Action::CopyPrimary)
        )
    } else {
        format!("{} to copy", model.prefs.chord(Action::CopyPrimary))
    };

    let content = column::with_capacity(4)
        .spacing(12)
        .push(header)
        .push(divider::horizontal::default())
        .push(body)
        .push(text::caption(reveal_hint));
    container(content)
        .padding([4, 8])
        .width(Length::Fixed(WIDTH))
        .into()
}
