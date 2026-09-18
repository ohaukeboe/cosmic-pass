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
    /// When the show request that is still waiting for its surface arrived (SC-001).
    pending_show: Option<Instant>,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            id: window::Id::unique(),
            visible: false,
            last_hide: None,
            pending_show: None,
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

    /// Starts the open-latency measurement (SC-001): `at` is when the show request arrived,
    /// which is earlier than the layer surface being asked for.
    pub fn mark_show_requested(&mut self, at: Instant) {
        self.pending_show = Some(at);
    }

    /// Time from the show request to now, once, if a measurement is pending. Called when the
    /// surface reports `LayerEvent::Focused`, which is when it can accept typing.
    pub fn take_open_latency(&mut self) -> Option<Duration> {
        self.pending_show
            .take()
            .map(|at| Instant::now().saturating_duration_since(at))
    }

    /// Marks the surface visible. Returns `false` when it already was, in which case no new
    /// layer surface — and so no `Focused` event — follows and any pending measurement is
    /// dropped rather than charged to a later open.
    fn begin_show(&mut self) -> bool {
        if self.visible {
            self.pending_show = None;
            return false;
        }
        self.visible = true;
        true
    }

    pub fn show<M: Clone + Send + 'static>(&mut self) -> Task<M>
    where
        CosmicPass: cosmic::Application<Message = M>,
    {
        if !self.begin_show() {
            return text_input::focus(SEARCH_INPUT.clone());
        }
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
        self.pending_show = None;
        destroy_layer_surface(self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ago(ms: u64) -> Instant {
        Instant::now() - Duration::from_millis(ms)
    }

    #[test]
    fn focus_without_a_show_request_measures_nothing() {
        let mut surface = Surface::default();
        assert!(surface.take_open_latency().is_none());
    }

    #[test]
    fn latency_is_measured_from_the_show_request_and_only_once() {
        let mut surface = Surface::default();
        surface.mark_show_requested(ago(40));
        let measured = surface.take_open_latency().expect("a pending measurement");
        assert!(measured >= Duration::from_millis(40), "{measured:?}");
        assert!(surface.take_open_latency().is_none());
    }

    #[test]
    fn a_new_request_replaces_an_older_pending_one() {
        let mut surface = Surface::default();
        surface.mark_show_requested(ago(5_000));
        surface.mark_show_requested(ago(10));
        let measured = surface.take_open_latency().expect("a pending measurement");
        assert!(measured < Duration::from_millis(1_000), "{measured:?}");
    }

    #[test]
    fn showing_an_already_visible_surface_drops_the_pending_measurement() {
        // No new layer surface is created, so no `Focused` event follows; a kept request
        // would later be charged to an unrelated open.
        let mut surface = Surface {
            visible: true,
            ..Default::default()
        };
        surface.mark_show_requested(ago(10));
        assert!(!surface.begin_show());
        assert!(surface.take_open_latency().is_none());
    }

    #[test]
    fn hiding_drops_the_pending_measurement() {
        let mut surface = Surface {
            visible: true,
            ..Default::default()
        };
        surface.mark_show_requested(ago(10));
        let _task: Task<()> = surface.hide();
        assert!(surface.take_open_latency().is_none());
    }
}
