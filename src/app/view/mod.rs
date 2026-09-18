//! Widgets for each UI mode.

use std::sync::LazyLock;

use cosmic::Element;
use cosmic::iced::{Border, Color, Length, Shadow};
use cosmic::widget::{Id, autosize, column, container, space};

use super::Message;
use crate::core::state::{Mode, Model};

pub mod actions;
pub mod detail;
pub mod list;
pub mod preferences;
pub mod status;

pub static RESULTS_ID: LazyLock<Id> = LazyLock::new(|| Id::new("results"));
static AUTOSIZE_ID: LazyLock<Id> = LazyLock::new(|| Id::new("autosize"));

/// The popup contents, framed like the COSMIC launcher.
pub fn popup(model: &Model, now: i64) -> Element<'_, Message> {
    let body = match &model.view.mode {
        Mode::Actions { selected, .. } => actions::view(model, *selected),
        Mode::Detail { .. } => detail::view(model, now),
        Mode::List | Mode::Preferences { .. } => {
            status::panel(model).unwrap_or_else(|| list::view(model))
        }
    };
    let framed = container(body)
        .padding([16, 20])
        .width(Length::Shrink)
        .height(Length::Shrink)
        .class(cosmic::theme::Container::custom(|theme| {
            let t = theme.cosmic();
            container::Style {
                text_color: Some(t.on_bg_color().into()),
                icon_color: Some(t.on_bg_color().into()),
                background: Some(Color::from(t.background(theme.transparent).base).into()),
                border: Border {
                    radius: t.radius_m().into(),
                    width: 1.0,
                    color: t.bg_divider().into(),
                },
                shadow: Shadow::default(),
                snap: true,
            }
        }));
    let window = column::with_capacity(2)
        .push(space::vertical().height(Length::Fixed(super::surface::TOP_MARGIN)))
        .push(framed);
    autosize::autosize(window, AUTOSIZE_ID.clone()).into()
}
