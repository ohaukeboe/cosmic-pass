//! Layer-shell popup surface: show, hide, and focus-loss handling.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use cosmic::app::Task;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{
    IcedMargin, SctkLayerSurfaceSettings,
};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, destroy_layer_surface,
};
use cosmic::iced::runtime::core::layout::Limits;
use cosmic::iced::window;
use cosmic::surface::action::{LiveSettings, app_layer_shell};
use cosmic::widget::{Id, text_input};

use super::CosmicPass;

pub static SEARCH_INPUT: LazyLock<Id> = LazyLock::new(|| Id::new("search"));

pub const WIDTH: f32 = 640.0;
/// Gap between the top of the output and the popup, in logical pixels.
pub const TOP_MARGIN: f32 = 160.0;
/// A toggle arriving this soon after a hide is ignored, so a focus loss caused by the
/// shortcut itself does not immediately re-open the window.
const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(100);

pub struct Surface {
    pub id: window::Id,
    visible: bool,
    last_hide: Option<Instant>,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            id: window::Id::unique(),
            visible: false,
            last_hide: None,
        }
    }
}

impl Surface {
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn recently_hidden(&self) -> bool {
        self.last_hide
            .is_some_and(|t| t.elapsed() < TOGGLE_DEBOUNCE)
    }

    pub fn show<M: Clone + Send + 'static>(&mut self) -> Task<M>
    where
        CosmicPass: cosmic::Application<Message = M>,
    {
        if self.visible {
            return text_input::focus(SEARCH_INPUT.clone());
        }
        self.visible = true;
        let id = self.id;
        cosmic::surface::surface_task(app_layer_shell(
            |_: &CosmicPass| LiveSettings {
                padding: Some(IcedMargin::default()),
                corners: None,
                blur: None,
            },
            move |_: &mut CosmicPass| SctkLayerSurfaceSettings {
                id,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor: Anchor::TOP,
                namespace: "cosmic-pass".into(),
                size: None,
                size_limits: Limits::NONE.min_width(1.0).min_height(1.0).max_width(WIDTH),
                exclusive_zone: -1,
                ..Default::default()
            },
            None,
        ))
        .chain(text_input::focus(SEARCH_INPUT.clone()))
    }

    pub fn hide<M: Send + 'static>(&mut self) -> Task<M> {
        if !self.visible {
            return Task::none();
        }
        self.visible = false;
        self.last_hide = Some(Instant::now());
        destroy_layer_surface(self.id)
    }
}
