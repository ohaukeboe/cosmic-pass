//! libcosmic application shell: thin adapter between the window and `core`.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cosmic::app::{Core, CosmicFlags, Settings, Task};
use cosmic::dbus_activation::Details;
use cosmic::iced::event::{self, listen_raw};
use cosmic::iced::keyboard::{self, Key, Modifiers};
use cosmic::iced::runtime::core::event::wayland::LayerEvent;
use cosmic::iced::runtime::core::event::{PlatformSpecific, wayland};
use cosmic::iced::widget::operation::snap_to;
use cosmic::iced::widget::scrollable::RelativeOffset;
use cosmic::iced::{Subscription, time, window};
use cosmic::{Element, task};
use serde::{Deserialize, Serialize};

use crate::clipboard::{Clipboard, ClipboardError, HelperClipboard};
use crate::config::{Modifier, Preferences};
use crate::core::effects::Effect;
use crate::core::state::{Model, Msg};
use crate::pass::backend::PassCli;
use crate::pass::runner::TokioRunner;
use crate::runtime::{Deps, Step};

pub mod keys;
pub mod surface;
pub mod view;

pub const APP_ID: &str = "io.github.ohaukeboe.CosmicPass";

/// Actions sent to a running instance over D-Bus (see `contracts/cli.md`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAction {
    Show,
    Hide,
    Refresh,
    /// Start without showing the window; a no-op for a running instance.
    Background,
}

impl std::fmt::Display for RemoteAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let json = serde_json::to_string(self).map_err(|_| std::fmt::Error)?;
        f.write_str(&json)
    }
}

#[derive(Debug, Clone)]
pub struct Flags {
    pub action: Option<RemoteAction>,
}

impl CosmicFlags for Flags {
    type SubCommand = RemoteAction;
    type Args = Vec<String>;

    fn action(&self) -> Option<&RemoteAction> {
        self.action.as_ref()
    }
}

pub fn run(flags: Flags) -> cosmic::iced::Result {
    cosmic::app::run_single_instance::<CosmicPass>(
        Settings::default()
            .no_main_window(true)
            .exit_on_close(false)
            .client_decorations(true),
        flags,
    )
}

#[derive(Debug, Clone)]
pub enum Message {
    Core(Msg),
    Key(keys::KeyPress),
    Layer(LayerEvent, window::Id),
    Submit,
    RowPressed(usize),
    OpenUrl(String),
    Nothing,
}

pub struct CosmicPass {
    core: Core,
    model: Model,
    deps: Deps,
    surface: surface::Surface,
    last_press: Option<(usize, Instant)>,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Used when the clipboard helper cannot be located; every copy fails visibly.
struct NoClipboard;

impl Clipboard for NoClipboard {
    fn copy(
        &self,
        _value: secrecy::SecretString,
        _secret: bool,
        _clear_after: Duration,
    ) -> crate::pass::runner::BoxFuture<'_, Result<(), ClipboardError>> {
        Box::pin(async { Err(ClipboardError::Unavailable) })
    }
}

fn load_preferences() -> Preferences {
    use cosmic::cosmic_config::CosmicConfigEntry;
    let config =
        match cosmic::cosmic_config::Config::new(crate::config::CONFIG_ID, Preferences::VERSION) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("preferences unavailable, using defaults: {e}");
                return Preferences::default();
            }
        };
    match Preferences::get_entry(&config) {
        Ok(p) => p.validated(),
        Err((errors, p)) => {
            for e in errors.iter().filter(|e| e.is_err()) {
                tracing::warn!("preferences: {e}");
            }
            p.validated()
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl CosmicPass {
    fn dispatch(&mut self, msg: Msg) -> Task<Message> {
        let effects = self.model.update(msg, now());
        Task::batch(effects.into_iter().map(|e| self.run_effect(e)))
    }

    fn run_effect(&mut self, effect: Effect) -> Task<Message> {
        match crate::runtime::execute(effect, &self.deps, &self.model) {
            Step::ShowWindow => {
                if self.surface.recently_hidden() {
                    // The shortcut press itself caused the focus loss that hid the window.
                    self.model.view.visible = false;
                    return Task::none();
                }
                self.surface.show()
            }
            Step::HideWindow => self.surface.hide(),
            Step::Future(f) => task::future(async move {
                match f.await {
                    Some(msg) => Message::Core(msg),
                    None => Message::Nothing,
                }
            }),
            Step::Stream(stream) => {
                use futures::StreamExt;
                task::stream(stream.map(Message::Core))
            }
            Step::Unhandled(effect) => {
                tracing::debug!(effect = effect_name(&effect), "effect skipped");
                Task::none()
            }
        }
    }

    fn on_key(&mut self, press: &keys::KeyPress) -> Task<Message> {
        // The caret position is not exposed by the text input; an empty query is the only
        // state where it is known to be at the end.
        let ctx = keys::KeyContext {
            caret_at_end: self.model.view.query.is_empty(),
        };
        match keys::map_key(press, &self.model.view.mode, &self.model.prefs, ctx) {
            Some(msg) => {
                let scroll = matches!(
                    msg,
                    Msg::SelectNext | Msg::SelectPrev | Msg::PageDown | Msg::PageUp
                );
                let task = self.dispatch(msg);
                if scroll {
                    task.chain(self.scroll_to_selection())
                } else {
                    task
                }
            }
            None => Task::none(),
        }
    }

    fn scroll_to_selection(&self) -> Task<Message> {
        let len = self.model.view.results.len();
        if len < 2 {
            return Task::none();
        }
        #[allow(clippy::cast_precision_loss)]
        let y = self.model.view.selected as f32 / (len - 1) as f32;
        snap_to(view::RESULTS_ID.clone(), RelativeOffset { x: 0.0, y })
    }
}

/// Variant name only; effects may carry secrets.
fn effect_name(effect: &Effect) -> &'static str {
    match effect {
        Effect::ShowWindow => "ShowWindow",
        Effect::HideWindow => "HideWindow",
        Effect::Refresh => "Refresh",
        Effect::ProbeSession => "ProbeSession",
        Effect::FetchAndCopy { .. } => "FetchAndCopy",
        Effect::FetchTotpAndCopy { .. } => "FetchTotpAndCopy",
        Effect::Copy { .. } => "Copy",
        Effect::Persist => "Persist",
        Effect::LoadCache => "LoadCache",
        Effect::DeleteCache => "DeleteCache",
        Effect::StartLogin => "StartLogin",
        Effect::FetchReveal { .. } => "FetchReveal",
        Effect::FetchTotp { .. } => "FetchTotp",
        Effect::ExpireNotice { .. } => "ExpireNotice",
        Effect::SavePreferences(_) => "SavePreferences",
    }
}

fn to_key_press(key: &Key, modifiers: Modifiers) -> Option<keys::KeyPress> {
    let name = match key {
        Key::Named(named) => format!("{named:?}"),
        Key::Character(c) => c.to_string(),
        Key::Unidentified => return None,
    };
    let mut mods = Vec::new();
    if modifiers.control() {
        mods.push(Modifier::Ctrl);
    }
    if modifiers.shift() {
        mods.push(Modifier::Shift);
    }
    if modifiers.alt() {
        mods.push(Modifier::Alt);
    }
    if modifiers.logo() {
        mods.push(Modifier::Super);
    }
    Some(keys::KeyPress {
        key: name,
        modifiers: mods,
    })
}

impl cosmic::Application for CosmicPass {
    type Executor = cosmic::executor::multi::Executor;
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, flags: Flags) -> (Self, Task<Message>) {
        core.set_keyboard_nav(false);
        let clipboard: Arc<dyn Clipboard> = match HelperClipboard::from_env() {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::error!("clipboard helper unavailable: {e}");
                Arc::new(NoClipboard)
            }
        };
        let keys: Arc<dyn crate::cache::keystore::KeyStore> =
            Arc::new(crate::cache::keystore::Oo7KeyStore);
        let deps = Deps {
            backend: Arc::new(PassCli::new(Arc::new(TokioRunner::from_env()))),
            clipboard,
            cache: Some(crate::cache::store::CacheStore::from_env(keys)),
            save_preferences: true,
        };
        let mut app = Self {
            core,
            model: Model::new(load_preferences()),
            deps,
            surface: surface::Surface::default(),
            last_press: None,
        };
        let mut tasks = vec![app.dispatch(Msg::Startup)];
        if !matches!(
            flags.action,
            Some(RemoteAction::Background | RemoteAction::Hide)
        ) {
            tasks.push(app.dispatch(Msg::Show));
        }
        (app, Task::batch(tasks))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Core(msg) => self.dispatch(msg),
            Message::Key(press) => self.on_key(&press),
            Message::Layer(LayerEvent::Unfocused, id) if id == self.surface.id => {
                self.dispatch(Msg::Hide)
            }
            Message::Layer(..) => Task::none(),
            // Enter is handled through the key map so it respects configured shortcuts.
            Message::Submit | Message::Nothing => Task::none(),
            Message::OpenUrl(url) => {
                let open = task::future(async move {
                    let status = tokio::process::Command::new("xdg-open")
                        .arg(&url)
                        .stdin(std::process::Stdio::null())
                        .status()
                        .await;
                    if let Err(e) = status {
                        tracing::warn!("could not open {url}: {e}");
                    }
                    Message::Nothing
                });
                Task::batch([open, self.dispatch(Msg::Hide)])
            }
            Message::RowPressed(i) => {
                let double = self
                    .last_press
                    .is_some_and(|(row, at)| row == i && at.elapsed() < DOUBLE_CLICK);
                self.last_press = Some((i, Instant::now()));
                let select = self.dispatch(Msg::Select(i));
                if double {
                    self.last_press = None;
                    select.chain(self.dispatch(Msg::CopyPrimary))
                } else {
                    select
                }
            }
        }
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Message> {
        match msg.msg {
            Details::Activate => {
                if self.surface.recently_hidden() {
                    return Task::none();
                }
                self.dispatch(Msg::Toggle)
            }
            Details::ActivateAction { action, .. } => {
                match serde_json::from_str::<RemoteAction>(&action) {
                    Ok(RemoteAction::Show) => self.dispatch(Msg::Show),
                    Ok(RemoteAction::Hide) => self.dispatch(Msg::Hide),
                    Ok(RemoteAction::Refresh) => self.dispatch(Msg::RefreshRequested),
                    Ok(RemoteAction::Background) => Task::none(),
                    Err(e) => {
                        tracing::warn!("ignoring unknown D-Bus action: {e}");
                        Task::none()
                    }
                }
            }
            Details::Open { .. } => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        cosmic::widget::space::vertical().into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if id == self.surface.id {
            view::popup(&self.model, now())
        } else {
            cosmic::widget::space::vertical().into()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let ticking = self.model.view.visible && self.model.view.totp.is_some();
        let tick = if ticking {
            time::every(Duration::from_secs(1)).map(|_| Message::Core(Msg::Tick(now())))
        } else {
            Subscription::none()
        };
        let events = listen_raw(|event, status, _window| match event {
            cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(
                wayland::Event::Layer(e, _, id),
            )) => Some(Message::Layer(e, id)),
            cosmic::iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key, modifiers, ..
            }) => {
                // Let the text field handle editing keys it consumed.
                let plain_char = matches!(key, Key::Character(_)) && !modifiers.control();
                if plain_char && status == event::Status::Captured {
                    return None;
                }
                to_key_press(&key, modifiers).map(Message::Key)
            }
            _ => None,
        });
        let prefs = cosmic::cosmic_config::config_subscription::<_, Preferences>(
            "preferences",
            crate::config::CONFIG_ID.into(),
            <Preferences as cosmic::cosmic_config::CosmicConfigEntry>::VERSION,
        )
        .map(|update| Message::Core(Msg::PrefsLoaded(update.config)));
        Subscription::batch([events, tick, prefs])
    }
}
