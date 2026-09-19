//! Widgets for each UI mode.

use std::sync::LazyLock;

use cosmic::Element;
use cosmic::iced::{Border, Color, Length, Shadow};
use cosmic::widget::{Id, autosize, column, container, space};

use super::Message;
use crate::core::state::{Mode, Model};

pub mod actions;
pub mod list;
pub mod preferences;
pub mod status;

pub static RESULTS_ID: LazyLock<Id> = LazyLock::new(|| Id::new("results"));
static AUTOSIZE_ID: LazyLock<Id> = LazyLock::new(|| Id::new("autosize"));

/// Which pane the popup shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// The action list, with the highlighted row.
    Actions(usize),
    Preferences,
    /// The result list, or the session panel standing in for it.
    List,
}

/// The pane a mode puts on screen. Only the result list is worth replacing with a session
/// panel: the other panes are opened deliberately, and the preferences editor in particular
/// stays useful — and is the only way back to the shortcuts — while the session is locked or
/// signed out (FR-027).
pub fn pane(mode: &Mode) -> Pane {
    match mode {
        Mode::Actions { selected, .. } => Pane::Actions(*selected),
        Mode::Preferences { .. } => Pane::Preferences,
        Mode::List => Pane::List,
    }
}

/// The caption lines a pane shows under its header: why the data may be out of date, then the
/// inline notice left by the last action. Shared, because a pane that renders neither leaves a
/// failed reveal or copy with nothing to show for it.
pub fn notices(model: &Model) -> Vec<&str> {
    let mut lines = Vec::with_capacity(2);
    lines.extend(model.stale_notice());
    lines.extend(model.view.notice.as_ref().map(|n| n.text.as_str()));
    lines
}

/// The popup contents, framed like the COSMIC launcher.
pub fn popup(model: &Model, now: i64) -> Element<'_, Message> {
    let body = match pane(&model.view.mode) {
        Pane::Actions(selected) => actions::view(model, selected, now),
        Pane::Preferences => preferences::view(model),
        Pane::List => status::panel(model).unwrap_or_else(|| list::view(model)),
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

#[cfg(test)]
mod tests {
    use super::{Pane, notices, pane};
    use crate::core::state::{Mode, Model};

    #[test]
    fn the_preferences_editor_is_a_pane_of_its_own() {
        assert_eq!(
            pane(&Mode::Preferences { rebinding: None }),
            Pane::Preferences,
            "opening the editor must not land on the result list (FR-027)"
        );
        assert_eq!(pane(&Mode::List), Pane::List);
    }

    /// The pane a mode is expected to show. Written as an exhaustive match so a `Mode`
    /// variant with no pane — the removed detail pane coming back, or a new screen added
    /// without one — fails to compile here rather than landing somewhere by accident.
    fn expected(mode: &Mode) -> Pane {
        match mode {
            Mode::List => Pane::List,
            Mode::Actions { selected, .. } => Pane::Actions(*selected),
            Mode::Preferences { .. } => Pane::Preferences,
        }
    }

    #[test]
    fn every_mode_shows_a_pane_and_none_of_them_is_a_detail_pane() {
        for mode in [
            Mode::List,
            Mode::Actions {
                key: crate::model::ItemKey::new("s", "a"),
                selected: 2,
            },
            Mode::Preferences { rebinding: None },
        ] {
            assert_eq!(pane(&mode), expected(&mode), "{mode:?}");
        }
    }

    #[test]
    fn a_pane_captions_the_stale_line_and_then_the_inline_notice() {
        let mut model = Model::default();
        model.notify("This item has no such field");
        assert_eq!(notices(&model), ["This item has no such field"]);
        model.data.stale = true;
        assert_eq!(
            notices(&model),
            [
                "Data may be out of date. Press F5 to refresh.",
                "This item has no such field"
            ]
        );
    }
}
