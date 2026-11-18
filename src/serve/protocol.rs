//! The wire types of `serve`: intents in, snapshots out. Pure: no IO, no clock.
//! Contract: `specs/004-serve-mode/contracts/serve-protocol.md`.

use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::core::actions::CopySource;
use crate::core::state::{Mode, Model, Msg, SessionState};
use crate::model::{ItemKind, ItemSummary};

/// Protocol version. A front end shows an error rather than guessing when it differs.
pub const VERSION: u8 = 1;

/// One line from the front end. Each maps to one reducer message. No `Debug`: a `query`
/// carries what the user typed.
#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Intent {
    Show,
    Hide,
    Query {
        text: String,
    },
    Next,
    Prev,
    PageDown,
    PageUp,
    Select {
        index: usize,
    },
    CopyPrimary,
    OpenFields,
    Activate {
        #[serde(default)]
        index: Option<usize>,
    },
    Reveal,
    Back,
    Escape,
    Refresh,
    Login,
}

impl From<Intent> for Msg {
    fn from(intent: Intent) -> Self {
        match intent {
            Intent::Show => Msg::Show,
            Intent::Hide => Msg::Hide,
            Intent::Query { text } => Msg::QueryChanged(text),
            Intent::Next => Msg::SelectNext,
            Intent::Prev => Msg::SelectPrev,
            Intent::PageDown => Msg::PageDown,
            Intent::PageUp => Msg::PageUp,
            Intent::Select { index } => Msg::Select(index),
            Intent::CopyPrimary => Msg::CopyPrimary,
            Intent::OpenFields => Msg::OpenActions,
            Intent::Activate { index } => Msg::ActivateAction(index),
            Intent::Reveal => Msg::ToggleReveal,
            Intent::Back => Msg::Back,
            Intent::Escape => Msg::Escape,
            Intent::Refresh => Msg::RefreshRequested,
            Intent::Login => Msg::StartLogin,
        }
    }
}

/// What the front end draws. Complete each time; the front end keeps only the last one.
/// No `Debug`: a revealed secret is in `fields` (plan.md, Complexity Tracking 1).
#[derive(Serialize)]
pub struct Snapshot {
    v: u8,
    visible: bool,
    session: &'static str,
    session_detail: Option<String>,
    can_login: bool,
    login: Option<Login>,
    mode: &'static str,
    query: String,
    selected: usize,
    rows: Vec<Row>,
    item: Option<Row>,
    fields: Vec<Field>,
    busy: Busy,
    stale: Option<&'static str>,
    notice: Option<String>,
    cli_warning: Option<String>,
}

#[derive(Serialize)]
struct Login {
    url: Option<String>,
    lines: Vec<String>,
}

#[derive(Serialize)]
struct Row {
    title: String,
    subtitle: Option<String>,
    vault: String,
    kind: &'static str,
}

#[derive(Serialize)]
struct Field {
    label: String,
    secret: bool,
    value: Option<String>,
    revealing: bool,
    totp: Option<Totp>,
}

#[derive(Serialize)]
struct Totp {
    code: String,
    remaining: u32,
}

#[derive(Serialize)]
struct Busy {
    refreshing: bool,
    fetching: bool,
    copying: bool,
}

impl Snapshot {
    pub fn of(model: &Model, now: i64) -> Self {
        let fields_mode = matches!(model.view.mode, Mode::Actions { .. });
        let selected = match model.view.mode {
            Mode::Actions { selected, .. } => selected,
            Mode::List | Mode::Preferences { .. } => model.view.selected,
        };
        let (session, session_detail) = session(&model.session);
        Self {
            v: VERSION,
            visible: model.view.visible,
            session,
            session_detail,
            can_login: model.can_start_login(),
            login: matches!(model.session, SessionState::LoggingIn).then(|| Login {
                url: model.login_url.clone(),
                lines: model.login_lines.clone(),
            }),
            mode: if fields_mode { "fields" } else { "list" },
            query: model.view.query.clone(),
            selected,
            rows: model
                .view
                .results
                .iter()
                .filter_map(|r| model.data.items.get(r.item))
                .map(row)
                .collect(),
            item: fields_mode.then(|| model.target_item().map(row)).flatten(),
            fields: if fields_mode {
                fields(model, now)
            } else {
                vec![]
            },
            busy: Busy {
                refreshing: model.data.refreshing,
                fetching: model.view.pending.is_some(),
                copying: model.view.copying,
            },
            stale: model.stale_notice(),
            notice: model.view.notice.as_ref().map(|n| n.text.clone()),
            cli_warning: model.cli_warning.clone(),
        }
    }
}

fn session(state: &SessionState) -> (&'static str, Option<String>) {
    match state {
        SessionState::Unknown => ("unknown", None),
        SessionState::Checking => ("checking", None),
        SessionState::SignedIn(_) => ("signed_in", None),
        SessionState::SignedOut => ("signed_out", None),
        SessionState::Locked => ("locked", None),
        SessionState::CliMissing => ("cli_missing", None),
        SessionState::LoggingIn => ("logging_in", None),
        SessionState::Error(text) => ("error", Some(text.clone())),
    }
}

fn row(item: &ItemSummary) -> Row {
    Row {
        title: item.display_title().to_owned(),
        subtitle: item.subtitle.clone(),
        vault: item.vault_name.clone(),
        kind: kind(&item.kind),
    }
}

fn kind(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::Login => "login",
        ItemKind::CreditCard => "card",
        ItemKind::Note => "note",
        ItemKind::Identity => "identity",
        ItemKind::Alias => "alias",
        ItemKind::Custom => "custom",
        ItemKind::SshKey | ItemKind::Wifi | ItemKind::Unknown(_) => "other",
    }
}

/// The same reads the popup's field list makes (`app/view/actions.rs`, research R11).
fn fields(model: &Model, now: i64) -> Vec<Field> {
    model
        .actions()
        .into_iter()
        .map(|entry| {
            let secret = entry.source.is_secret();
            match &entry.source {
                CopySource::Totp { field } => {
                    let totp = model
                        .revealed_totp()
                        .filter(|t| t.field == *field)
                        .map(|t| Totp {
                            code: t.code.expose_secret().to_owned(),
                            remaining: t.remaining(now),
                        });
                    Field {
                        label: entry.label,
                        secret,
                        value: None,
                        revealing: model.revealing_totp(),
                        totp,
                    }
                }
                CopySource::Field(f) => {
                    let value = if f.secret {
                        entry
                            .field_index
                            .and_then(|i| model.revealed_value(i))
                            .map(|v| v.expose_secret().to_owned())
                    } else {
                        f.value.clone()
                    };
                    Field {
                        label: entry.label,
                        secret,
                        value,
                        revealing: entry.field_index.is_some_and(|i| model.revealing_field(i)),
                        totp: None,
                    }
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Msg {
        Msg::from(serde_json::from_str::<Intent>(line).expect(line))
    }

    #[test]
    fn every_intent_reaches_its_message() {
        let cases: [(&str, &str); 16] = [
            (r#"{"t":"show"}"#, "Show"),
            (r#"{"t":"hide"}"#, "Hide"),
            (r#"{"t":"query","text":"gh"}"#, r#"QueryChanged("gh")"#),
            (r#"{"t":"next"}"#, "SelectNext"),
            (r#"{"t":"prev"}"#, "SelectPrev"),
            (r#"{"t":"page_down"}"#, "PageDown"),
            (r#"{"t":"page_up"}"#, "PageUp"),
            (r#"{"t":"select","index":3}"#, "Select(3)"),
            (r#"{"t":"copy_primary"}"#, "CopyPrimary"),
            (r#"{"t":"open_fields"}"#, "OpenActions"),
            (r#"{"t":"activate","index":null}"#, "ActivateAction(None)"),
            (r#"{"t":"reveal"}"#, "ToggleReveal"),
            (r#"{"t":"back"}"#, "Back"),
            (r#"{"t":"escape"}"#, "Escape"),
            (r#"{"t":"refresh"}"#, "RefreshRequested"),
            (r#"{"t":"login"}"#, "StartLogin"),
        ];
        for (line, msg) in cases {
            assert_eq!(format!("{:?}", parse(line)), msg, "{line}");
        }
    }

    #[test]
    fn activate_index_is_optional_and_extra_fields_are_ignored() {
        assert_eq!(
            format!("{:?}", parse(r#"{"t":"activate"}"#)),
            "ActivateAction(None)"
        );
        assert_eq!(
            format!("{:?}", parse(r#"{"t":"activate","index":2,"x":1}"#)),
            "ActivateAction(Some(2))"
        );
    }

    #[test]
    fn unknown_or_malformed_intents_do_not_parse() {
        for line in [r#"{"t":"nope"}"#, r#"{"t":"select"}"#, "garbage", "{}"] {
            assert!(serde_json::from_str::<Intent>(line).is_err(), "{line}");
        }
    }
}
