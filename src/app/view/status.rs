//! Session and error status panels.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, icon, text};

use crate::app::Message;
use crate::app::surface::WIDTH;
use crate::config::Action;
use crate::core::state::{Model, Msg, PASS_CLI_URL, SessionState};

/// A full panel for session states that make the item list meaningless, if any.
pub fn panel(model: &Model) -> Option<Element<'_, Message>> {
    let (icon_name, title, detail): (&str, &str, String) = match &model.session {
        SessionState::SignedOut => (
            "system-lock-screen-symbolic",
            "Not signed in",
            "Sign in to Proton Pass to search your items.".into(),
        ),
        SessionState::LoggingIn => (
            "view-refresh-symbolic",
            "Waiting for browser sign-in…",
            "Finish signing in in your browser.".into(),
        ),
        SessionState::Locked => (
            "system-lock-screen-symbolic",
            "Session locked",
            "Run `pass-cli session unlock` in a terminal, then try again.".into(),
        ),
        SessionState::CliMissing => (
            "dialog-warning-symbolic",
            "pass-cli is not installed",
            "COSMIC Pass needs the Proton Pass command-line tool.".to_owned(),
        ),
        SessionState::Error(message) => (
            "dialog-warning-symbolic",
            "Something went wrong",
            message.clone(),
        ),
        SessionState::Checking if model.data.items.is_empty() => (
            "view-refresh-symbolic",
            "Checking Proton Pass…",
            String::new(),
        ),
        SessionState::Checking | SessionState::Unknown | SessionState::SignedIn(_) => {
            return None;
        }
    };

    let mut content = column::with_capacity(6)
        .spacing(12)
        .align_x(Alignment::Center)
        .push(icon::from_name(icon_name).size(48))
        .push(text::title3(title));
    if !detail.is_empty() {
        content = content.push(text::body(detail));
    }
    if let Some(url) = &model.login_url {
        content = content.push(
            button::link(url.clone())
                .on_press(Message::OpenUrl(url.clone()))
                .trailing_icon(true),
        );
    }
    if model.session == SessionState::CliMissing {
        content = content
            .push(button::link(PASS_CLI_URL).on_press(Message::OpenUrl(PASS_CLI_URL.into())));
    }
    // The panel has no focus ring (keyboard navigation is off), so each button names its chord.
    if model.can_start_login() {
        content = content.push(
            button::suggested(format!("Sign in ({})", model.prefs.chord(Action::SignIn)))
                .on_press(Message::Core(Msg::StartLogin)),
        );
    }
    if matches!(
        model.session,
        SessionState::Locked | SessionState::CliMissing | SessionState::Error(_)
    ) {
        content = content.push(
            button::standard(format!("Try again ({})", model.prefs.chord(Action::Retry)))
                .on_press(Message::Core(Msg::Startup)),
        );
    }

    Some(
        container(content)
            .padding(24)
            .width(Length::Fixed(WIDTH))
            .align_x(Alignment::Center)
            .into(),
    )
}
