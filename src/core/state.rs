//! Application state and the reducer that turns messages into effects.

use secrecy::SecretString;
use tokio_util::sync::CancellationToken;

use super::actions::{
    ActionEntry, CopySource, all_actions, primary_action, totp_action, url_action, username_action,
};
use super::effects::Effect;
use super::search::{ResultRow, SearchIndex};
use super::totp::TotpDisplay;
use super::usage::UsageTable;
use crate::config::{Action, KeyChord, Preferences};
use crate::model::{AccountId, CACHE_FORMAT_VERSION, CacheFile, ItemKey, ItemSummary, Vault};
use crate::pass::backend::Listing;
use crate::pass::error::PassError;

const PAGE: usize = 10;

/// Where users can get `pass-cli`.
pub const PASS_CLI_URL: &str = "https://protonpass.github.io/pass-cli/";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SessionState {
    #[default]
    Unknown,
    Checking,
    SignedIn(AccountId),
    SignedOut,
    Locked,
    CliMissing,
    LoggingIn,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataSource {
    #[default]
    Memory,
    DiskCache,
}

#[derive(Debug, Default)]
pub struct DataState {
    pub vaults: Vec<Vault>,
    pub items: Vec<ItemSummary>,
    /// Unix seconds of the last successful full refresh.
    pub fetched_at: Option<i64>,
    pub stale: bool,
    pub refreshing: bool,
    pub source: DataSource,
    /// Refresh time recorded in the loaded cache file.
    pub cached_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    List,
    Actions {
        key: ItemKey,
        selected: usize,
    },
    Detail {
        key: ItemKey,
    },
    Preferences {
        /// The action waiting for a new key chord.
        rebinding: Option<Action>,
    },
}

#[derive(Debug)]
pub struct PendingFetch {
    pub key: ItemKey,
    pub cancel: CancellationToken,
    /// Whether the fetched value is secret (clipboard timeout).
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: u64,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct ViewState {
    pub visible: bool,
    pub query: String,
    pub results: Vec<ResultRow>,
    pub selected: usize,
    pub mode: Mode,
    pub pending: Option<PendingFetch>,
    pub revealed: Option<SecretString>,
    pub totp: Option<TotpDisplay>,
    /// A detail-pane fetch (TOTP or reveal) is in flight; cancelled when leaving the pane.
    pub detail_cancel: Option<CancellationToken>,
    pub totp_fetching: bool,
    pub notice: Option<Notice>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Toggle,
    Show,
    Hide,
    QueryChanged(String),
    SelectNext,
    SelectPrev,
    PageDown,
    PageUp,
    Select(usize),
    RefreshRequested,
    DataLoaded(Listing),
    RefreshFailed(PassError),
    NoticeExpired(u64),
    /// Copy the selected item's primary field (Enter).
    CopyPrimary,
    /// A field or code fetched for copying.
    CopyFetched {
        key: ItemKey,
        result: Result<SecretString, PassError>,
    },
    /// The clipboard accepted (or rejected) a copy.
    CopyFinished(Result<(), String>),
    /// Escape: cancel a pending fetch, leave a sub-mode, or hide.
    Escape,
    CopyUsername,
    CopyTotp,
    CopyUrl,
    /// Show every copyable field of the selected item.
    OpenActions,
    /// Copy an action-list entry: the highlighted one, or the given index.
    ActivateAction(Option<usize>),
    /// Leave a sub-mode without hiding.
    Back,
    /// App start: check the session (and load the cache).
    Startup,
    SessionProbed(Result<AccountId, PassError>),
    StartLogin,
    /// One output line of `pass-cli login`.
    LoginLine(String),
    LoginFinished(Result<(), PassError>),
    /// Show the detail pane for the selected item.
    OpenDetail,
    ToggleReveal,
    RevealFetched {
        key: ItemKey,
        result: Result<SecretString, PassError>,
    },
    TotpFetched {
        key: ItemKey,
        result: Result<std::collections::BTreeMap<String, SecretString>, PassError>,
    },
    /// Clock tick (Unix seconds) while the detail pane shows a code.
    Tick(i64),
    CacheLoaded(Option<CacheFile>),
    OpenPreferences,
    /// Change the clipboard timeout by this many seconds.
    AdjustClipboardClear(i64),
    StartRebind(Action),
    /// A key chord pressed while rebinding.
    ChordCaptured(KeyChord),
    ResetShortcuts,
    /// Preferences read from disk (initially or after an external change).
    PrefsLoaded(Preferences),
}

#[derive(Debug, Default)]
pub struct Model {
    pub session: SessionState,
    pub data: DataState,
    pub view: ViewState,
    pub prefs: Preferences,
    pub usage: UsageTable,
    /// URL printed by `pass-cli login`, shown while signing in.
    pub login_url: Option<String>,
    /// Account the loaded cache belongs to.
    cache_account: Option<AccountId>,
    index: SearchIndex,
    next_notice: u64,
}

impl Model {
    pub fn new(prefs: Preferences) -> Self {
        Self {
            prefs,
            ..Self::default()
        }
    }

    /// Everything worth caching, if an account is known and data was fetched.
    pub fn cache_snapshot(&self) -> Option<CacheFile> {
        let SessionState::SignedIn(account) = &self.session else {
            return None;
        };
        let fetched_at = match self.data.source {
            DataSource::Memory => self.data.fetched_at?,
            DataSource::DiskCache => self.data.cached_at?,
        };
        Some(CacheFile {
            format_version: CACHE_FORMAT_VERSION,
            account: account.clone(),
            fetched_at,
            vaults: self.data.vaults.clone(),
            items: self.data.items.clone(),
            usage: self.usage.records(),
        })
    }

    /// Whether the sign-in action is offered.
    pub fn can_start_login(&self) -> bool {
        matches!(
            self.session,
            SessionState::SignedOut | SessionState::Error(_)
        )
    }

    /// Whether item data can be fetched in the current session state.
    fn may_refresh(&self) -> bool {
        !matches!(
            self.session,
            SessionState::Checking
                | SessionState::SignedOut
                | SessionState::Locked
                | SessionState::CliMissing
                | SessionState::LoggingIn
        )
    }

    /// The item a sub-mode is showing, else the selected result.
    pub fn target_item(&self) -> Option<&ItemSummary> {
        match &self.view.mode {
            Mode::Actions { key, .. } | Mode::Detail { key } => self.item(key),
            Mode::List | Mode::Preferences { .. } => self.selected_item(),
        }
    }

    /// Action-list entries for the target item.
    pub fn actions(&self) -> Vec<ActionEntry> {
        self.target_item().map(all_actions).unwrap_or_default()
    }

    /// The item behind the selected result row.
    pub fn selected_item(&self) -> Option<&ItemSummary> {
        let row = self.view.results.get(self.view.selected)?;
        self.data.items.get(row.item)
    }

    pub fn item(&self, key: &ItemKey) -> Option<&ItemSummary> {
        self.data.items.iter().find(|i| &i.key == key)
    }

    pub fn update(&mut self, msg: Msg, now: i64) -> Vec<Effect> {
        match msg {
            Msg::Toggle if self.view.visible => self.hide(),
            Msg::Toggle | Msg::Show => self.show(now),
            Msg::Hide => self.hide(),
            Msg::QueryChanged(query) => {
                self.view.query = query;
                self.view.selected = 0;
                self.recompute_results();
                vec![]
            }
            Msg::SelectNext => self.move_selection(1),
            Msg::SelectPrev => self.move_selection(-1),
            Msg::PageDown => self.move_selection(PAGE as isize),
            Msg::PageUp => self.move_selection(-(PAGE as isize)),
            Msg::Select(index) => {
                if index < self.view.results.len() {
                    self.view.selected = index;
                }
                vec![]
            }
            Msg::RefreshRequested => self.request_refresh(),
            Msg::DataLoaded(listing) => {
                self.data.refreshing = false;
                self.data.stale = false;
                self.data.source = DataSource::Memory;
                self.data.fetched_at = Some(now);
                self.data.cached_at = None;
                self.cache_account = None;
                self.replace_items(listing);
                vec![Effect::Persist]
            }
            Msg::CacheLoaded(file) => self.cache_loaded(file),
            Msg::OpenPreferences => {
                self.leave_mode();
                self.view.mode = Mode::Preferences { rebinding: None };
                vec![]
            }
            Msg::AdjustClipboardClear(delta) => {
                let current = i64::from(self.prefs.clipboard_clear_secs);
                let next = u32::try_from(current.saturating_add(delta).max(0)).unwrap_or(u32::MAX);
                self.save_prefs(Preferences {
                    clipboard_clear_secs: next,
                    ..self.prefs.clone()
                })
            }
            Msg::StartRebind(action) => {
                if let Mode::Preferences { rebinding } = &mut self.view.mode {
                    *rebinding = Some(action);
                }
                vec![]
            }
            Msg::ChordCaptured(chord) => self.chord_captured(chord),
            Msg::ResetShortcuts => {
                let defaults = Preferences::default().shortcuts;
                self.save_prefs(Preferences {
                    shortcuts: defaults,
                    ..self.prefs.clone()
                })
            }
            Msg::PrefsLoaded(prefs) => {
                self.apply_prefs(prefs.validated());
                vec![]
            }
            Msg::RefreshFailed(error) => {
                self.data.refreshing = false;
                self.data.stale = true;
                self.session_error(error)
            }
            Msg::Startup => {
                if !matches!(
                    self.session,
                    SessionState::LoggingIn | SessionState::SignedIn(_)
                ) {
                    self.session = SessionState::Checking;
                }
                vec![Effect::LoadCache, Effect::ProbeSession]
            }
            Msg::SessionProbed(Ok(account)) => self.signed_in(account),
            Msg::SessionProbed(Err(error)) => {
                let effects = self.session_error(error.clone());
                if matches!(self.session, SessionState::Checking | SessionState::Unknown) {
                    self.session = match error {
                        PassError::Network | PassError::Timeout | PassError::Cancelled => {
                            SessionState::Unknown
                        }
                        other => SessionState::Error(other.to_string()),
                    };
                }
                effects
            }
            Msg::StartLogin => {
                if !self.can_start_login() {
                    return vec![];
                }
                self.session = SessionState::LoggingIn;
                self.login_url = None;
                vec![Effect::StartLogin]
            }
            Msg::LoginLine(line) => {
                if self.login_url.is_none() {
                    self.login_url = line
                        .split_whitespace()
                        .find(|w| w.starts_with("https://"))
                        .map(str::to_owned);
                }
                vec![]
            }
            Msg::LoginFinished(result) => {
                self.login_url = None;
                match result {
                    Ok(()) => {
                        self.session = SessionState::Checking;
                        vec![Effect::ProbeSession]
                    }
                    Err(e) => {
                        self.session = SessionState::Error(e.to_string());
                        vec![]
                    }
                }
            }
            Msg::CopyPrimary => self.copy_with(primary_action, "Nothing to copy", now),
            Msg::CopyUsername => self.copy_with(username_action, "This item has no username", now),
            Msg::CopyTotp => self.copy_with(totp_action, "This item has no one-time code", now),
            Msg::CopyUrl => self.copy_with(url_action, "This item has no website", now),
            Msg::OpenActions => {
                if let Some(key) = self.selected_item().map(|i| i.key.clone()) {
                    self.view.mode = Mode::Actions { key, selected: 0 };
                }
                vec![]
            }
            Msg::ActivateAction(index) => {
                let Mode::Actions { key, selected } = &self.view.mode else {
                    return vec![];
                };
                let (key, index) = (key.clone(), index.unwrap_or(*selected));
                match self.actions().into_iter().nth(index) {
                    Some(entry) => self.start_copy(key, entry.source, now),
                    None => vec![],
                }
            }
            Msg::Back => {
                self.leave_mode();
                vec![]
            }
            Msg::OpenDetail => self.open_detail(),
            Msg::ToggleReveal => self.toggle_reveal(),
            Msg::RevealFetched { key, result } => {
                if !self.in_detail(&key) {
                    return vec![];
                }
                match result {
                    Ok(value) => {
                        self.view.revealed = Some(value);
                        vec![]
                    }
                    Err(PassError::Cancelled) => vec![],
                    Err(e) => vec![self.notify(e.to_string())],
                }
            }
            Msg::TotpFetched { key, result } => {
                if !self.in_detail(&key) {
                    return vec![];
                }
                self.view.totp_fetching = false;
                match result {
                    Ok(mut codes) => {
                        let fields = self
                            .item(&key)
                            .map(|i| i.totp_fields.clone())
                            .unwrap_or_default();
                        self.view.totp = fields.into_iter().find_map(|field| {
                            let code = codes.remove(&field).or_else(|| {
                                (field == "totp_uri")
                                    .then(|| codes.remove("totp"))
                                    .flatten()
                            })?;
                            Some(TotpDisplay::new(field, code, now))
                        });
                        vec![]
                    }
                    Err(PassError::Cancelled) => vec![],
                    Err(e) => vec![self.notify(e.to_string())],
                }
            }
            Msg::Tick(now) => {
                let expired = self
                    .view
                    .totp
                    .as_ref()
                    .is_some_and(|t| t.needs_refresh(now));
                if expired && !self.view.totp_fetching {
                    self.fetch_totp()
                } else {
                    vec![]
                }
            }
            Msg::CopyFetched { key, result } => self.copy_fetched(&key, result, now),
            Msg::CopyFinished(Ok(())) => vec![],
            Msg::CopyFinished(Err(_)) => vec![self.notify("Clipboard unavailable")],
            Msg::Escape => self.escape(),
            Msg::NoticeExpired(id) => {
                if self.view.notice.as_ref().is_some_and(|n| n.id == id) {
                    self.view.notice = None;
                }
                vec![]
            }
        }
    }

    /// Shows an inline notice and returns the effect that expires it.
    pub fn notify(&mut self, text: impl Into<String>) -> Effect {
        self.next_notice += 1;
        let id = self.next_notice;
        self.view.notice = Some(Notice {
            id,
            text: text.into(),
        });
        Effect::ExpireNotice {
            id,
            after: NOTICE_DURATION,
        }
    }

    fn apply_prefs(&mut self, prefs: Preferences) {
        let reindex = prefs.max_results != self.prefs.max_results;
        self.prefs = prefs;
        if reindex {
            self.recompute_results();
        }
    }

    fn save_prefs(&mut self, prefs: Preferences) -> Vec<Effect> {
        let prefs = prefs.validated();
        if prefs == self.prefs {
            return vec![];
        }
        self.apply_prefs(prefs.clone());
        vec![Effect::SavePreferences(prefs)]
    }

    fn chord_captured(&mut self, chord: KeyChord) -> Vec<Effect> {
        let Mode::Preferences {
            rebinding: Some(action),
        } = self.view.mode
        else {
            return vec![];
        };
        if let Some(other) = self.prefs.conflict(action, &chord) {
            return vec![self.notify(format!("{chord} is already used by “{}”", other.label()))];
        }
        self.view.mode = Mode::Preferences { rebinding: None };
        let mut prefs = self.prefs.clone();
        prefs.shortcuts.insert(action, chord);
        self.save_prefs(prefs)
    }

    fn cache_loaded(&mut self, file: Option<CacheFile>) -> Vec<Effect> {
        let Some(file) = file else {
            return vec![];
        };
        if self.data.fetched_at.is_some() {
            return vec![];
        }
        if matches!(&self.session, SessionState::SignedIn(a) if *a != file.account) {
            return vec![Effect::DeleteCache];
        }
        self.data.stale = true;
        self.data.source = DataSource::DiskCache;
        self.data.cached_at = Some(file.fetched_at);
        self.cache_account = Some(file.account);
        self.usage = UsageTable::from_records(file.usage);
        self.replace_items(crate::pass::backend::Listing {
            vaults: file.vaults,
            items: file.items,
        });
        vec![]
    }

    fn signed_in(&mut self, account: AccountId) -> Vec<Effect> {
        let mut effects = Vec::new();
        let other_session =
            matches!(&self.session, SessionState::SignedIn(previous) if *previous != account);
        let other_cache = self.cache_account.as_ref().is_some_and(|c| *c != account);
        if other_session || other_cache {
            self.clear_data();
            effects.push(Effect::DeleteCache);
        }
        self.session = SessionState::SignedIn(account);
        effects.extend(self.request_refresh());
        effects
    }

    /// Applies an error that may say something about the session.
    fn session_error(&mut self, error: PassError) -> Vec<Effect> {
        match error {
            PassError::SignedOut => {
                self.session = SessionState::SignedOut;
                self.clear_data();
                vec![Effect::DeleteCache]
            }
            PassError::Locked => {
                self.session = SessionState::Locked;
                vec![]
            }
            PassError::CliMissing => {
                self.session = SessionState::CliMissing;
                vec![]
            }
            _ => {
                self.data.stale = true;
                vec![]
            }
        }
    }

    /// Forgets all account data (sign-out or account switch).
    fn clear_data(&mut self) {
        let refreshing = self.data.refreshing;
        self.data = DataState {
            refreshing,
            ..DataState::default()
        };
        self.usage = UsageTable::default();
        self.cache_account = None;
        self.index = SearchIndex::default();
        self.view.selected = 0;
        self.recompute_results();
    }

    fn copy_with(
        &mut self,
        pick: fn(&ItemSummary) -> Option<CopySource>,
        missing: &str,
        now: i64,
    ) -> Vec<Effect> {
        let Some(item) = self.target_item() else {
            return vec![];
        };
        let key = item.key.clone();
        match pick(item) {
            Some(source) => self.start_copy(key, source, now),
            None => vec![self.notify(missing)],
        }
    }

    fn leave_mode(&mut self) {
        self.view.mode = Mode::List;
        self.view.revealed = None;
        self.view.totp = None;
        self.view.totp_fetching = false;
        if let Some(cancel) = self.view.detail_cancel.take() {
            cancel.cancel();
        }
    }

    fn in_detail(&self, key: &ItemKey) -> bool {
        matches!(&self.view.mode, Mode::Detail { key: k } if k == key)
    }

    fn detail_token(&mut self) -> CancellationToken {
        self.view
            .detail_cancel
            .get_or_insert_with(CancellationToken::new)
            .clone()
    }

    fn open_detail(&mut self) -> Vec<Effect> {
        let Some(item) = self.selected_item() else {
            return vec![];
        };
        let (key, has_totp) = (item.key.clone(), item.has_totp());
        self.leave_mode();
        self.view.mode = Mode::Detail { key };
        if has_totp { self.fetch_totp() } else { vec![] }
    }

    fn fetch_totp(&mut self) -> Vec<Effect> {
        let Mode::Detail { key } = &self.view.mode else {
            return vec![];
        };
        let key = key.clone();
        self.view.totp_fetching = true;
        vec![Effect::FetchTotp {
            key,
            cancel: self.detail_token(),
        }]
    }

    fn toggle_reveal(&mut self) -> Vec<Effect> {
        let Mode::Detail { key } = &self.view.mode else {
            return vec![];
        };
        if self.view.revealed.take().is_some() {
            return vec![];
        }
        let key = key.clone();
        let field = self.item(&key).and_then(|item| match primary_action(item) {
            Some(CopySource::Field(f)) if f.secret => Some(f.name),
            _ => None,
        });
        match field {
            Some(field) => vec![Effect::FetchReveal {
                key,
                field,
                cancel: self.detail_token(),
            }],
            None => vec![],
        }
    }

    fn start_copy(&mut self, key: ItemKey, source: CopySource, now: i64) -> Vec<Effect> {
        self.cancel_pending();
        let secret = source.is_secret();
        let cancel = CancellationToken::new();
        let effect = match source {
            CopySource::Field(field) => {
                if let Some(value) = field.value {
                    let mut effects = vec![Effect::Copy {
                        value: SecretString::from(value),
                        secret,
                    }];
                    effects.extend(self.finish_copy(&key, now));
                    return effects;
                }
                Effect::FetchAndCopy {
                    key: key.clone(),
                    field: field.name,
                    secret,
                    cancel: cancel.clone(),
                }
            }
            CopySource::Totp { field } => Effect::FetchTotpAndCopy {
                key: key.clone(),
                field,
                cancel: cancel.clone(),
            },
        };
        self.view.pending = Some(PendingFetch {
            key,
            cancel,
            secret,
        });
        vec![effect]
    }

    fn copy_fetched(
        &mut self,
        key: &ItemKey,
        result: Result<SecretString, PassError>,
        now: i64,
    ) -> Vec<Effect> {
        let Some(pending) = self.view.pending.take_if(|p| &p.key == key) else {
            return vec![];
        };
        match result {
            Ok(value) => {
                let mut effects = vec![Effect::Copy {
                    value,
                    secret: pending.secret,
                }];
                effects.extend(self.finish_copy(key, now));
                effects
            }
            Err(PassError::Cancelled) => vec![],
            Err(PassError::NotFound) => {
                let mut effects = vec![self.notify("Item no longer exists")];
                effects.extend(self.request_refresh());
                effects
            }
            Err(PassError::FieldMissing) => vec![self.notify("This field is empty")],
            Err(e) => {
                let mut effects = vec![self.notify(e.to_string())];
                effects.extend(self.session_error(e));
                effects
            }
        }
    }

    /// Records usage and hides the window after a successful copy.
    fn finish_copy(&mut self, key: &ItemKey, now: i64) -> Vec<Effect> {
        self.usage.record(key, now);
        let mut effects = vec![Effect::Persist];
        effects.extend(self.hide());
        effects
    }

    fn cancel_pending(&mut self) {
        if let Some(pending) = self.view.pending.take() {
            pending.cancel.cancel();
        }
    }

    fn escape(&mut self) -> Vec<Effect> {
        if self.view.pending.is_some() {
            self.cancel_pending();
            return vec![];
        }
        if let Mode::Preferences { rebinding } = &mut self.view.mode
            && rebinding.is_some()
        {
            *rebinding = None;
            return vec![];
        }
        if self.view.mode != Mode::List {
            self.leave_mode();
            return vec![];
        }
        self.hide()
    }

    fn show(&mut self, now: i64) -> Vec<Effect> {
        if self.view.visible {
            return vec![];
        }
        self.view.visible = true;
        let mut effects = vec![Effect::ShowWindow];
        let stale_after = i64::from(self.prefs.refresh_stale_secs);
        let stale = self
            .data
            .fetched_at
            .is_none_or(|at| now.saturating_sub(at) >= stale_after);
        if stale && self.may_refresh() {
            effects.extend(self.request_refresh());
        }
        effects
    }

    fn hide(&mut self) -> Vec<Effect> {
        self.cancel_pending();
        self.leave_mode();
        self.view = ViewState::default();
        self.recompute_results();
        vec![Effect::HideWindow]
    }

    fn request_refresh(&mut self) -> Vec<Effect> {
        if self.data.refreshing || !self.may_refresh() {
            return vec![];
        }
        self.data.refreshing = true;
        vec![Effect::Refresh]
    }

    fn move_selection(&mut self, delta: isize) -> Vec<Effect> {
        if let Mode::Actions { selected, .. } = self.view.mode {
            let len = self.actions().len();
            if let Mode::Actions { selected: s, .. } = &mut self.view.mode {
                *s = selected
                    .saturating_add_signed(delta)
                    .min(len.saturating_sub(1));
            }
            return vec![];
        }
        let len = self.view.results.len();
        if len > 0 {
            self.view.selected = self.view.selected.saturating_add_signed(delta).min(len - 1);
        }
        vec![]
    }

    /// Replaces the item list, keeping the selected item selected if it still exists.
    fn replace_items(&mut self, listing: Listing) {
        let selected_key = self.selected_item().map(|i| i.key.clone());
        self.data.vaults = listing.vaults;
        self.data.items = listing.items;
        self.usage.prune(&self.data.items);
        self.index = SearchIndex::build(&self.data.items);
        self.recompute_results();
        self.view.selected = selected_key
            .and_then(|key| {
                self.view
                    .results
                    .iter()
                    .position(|r| self.data.items[r.item].key == key)
            })
            .unwrap_or(0);
    }

    fn recompute_results(&mut self) {
        let usage = &self.usage;
        self.view.results = self.index.search(
            &self.view.query,
            |k| usage.recency(k),
            &self.data.items,
            usize::from(self.prefs.max_results),
        );
        let len = self.view.results.len();
        self.view.selected = self.view.selected.min(len.saturating_sub(1));
    }
}

const NOTICE_DURATION: std::time::Duration = std::time::Duration::from_secs(3);

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{ItemKind, ShareId};

    pub(crate) fn login(id: &str, title: &str) -> ItemSummary {
        ItemSummary {
            key: ItemKey::new("s", id),
            vault_name: "Personal".into(),
            kind: ItemKind::Login,
            title: title.into(),
            username: None,
            email: None,
            subtitle: None,
            urls: vec![],
            totp_fields: vec![],
            fields: vec![],
            modified_at: 0,
        }
    }

    pub(crate) fn listing(items: Vec<ItemSummary>) -> Listing {
        Listing {
            vaults: vec![Vault {
                share_id: ShareId("s".into()),
                name: "Personal".into(),
            }],
            items,
        }
    }

    pub(crate) fn loaded(items: Vec<ItemSummary>) -> Model {
        let mut m = Model::default();
        m.update(Msg::DataLoaded(listing(items)), 1_000);
        m
    }

    fn titles(m: &Model) -> Vec<String> {
        m.view
            .results
            .iter()
            .map(|r| m.data.items[r.item].title.clone())
            .collect()
    }

    fn many(n: usize) -> Vec<ItemSummary> {
        (0..n)
            .map(|i| login(&i.to_string(), &format!("item {i:02}")))
            .collect()
    }

    #[test]
    fn toggle_shows_then_hides() {
        let mut m = Model::default();
        let fx = m.update(Msg::Toggle, 0);
        assert!(m.view.visible);
        assert!(matches!(fx[..], [Effect::ShowWindow, Effect::Refresh]));
        let fx = m.update(Msg::Toggle, 0);
        assert!(!m.view.visible);
        assert!(matches!(fx[..], [Effect::HideWindow]));
    }

    #[test]
    fn show_when_visible_does_nothing() {
        let mut m = Model::default();
        m.update(Msg::Show, 0);
        assert!(m.update(Msg::Show, 0).is_empty());
    }

    #[test]
    fn query_change_filters_and_resets_selection() {
        let mut m = loaded(vec![login("a", "GitHub"), login("b", "Bank")]);
        m.update(Msg::SelectNext, 0);
        assert_eq!(m.view.selected, 1);
        m.update(Msg::QueryChanged("bank".into()), 0);
        assert_eq!(titles(&m), ["Bank"]);
        assert_eq!(m.view.selected, 0);
        assert_eq!(m.view.query, "bank");
    }

    #[test]
    fn selection_is_clamped() {
        let mut m = loaded(many(15));
        m.update(Msg::SelectPrev, 0);
        assert_eq!(m.view.selected, 0);
        m.update(Msg::PageDown, 0);
        assert_eq!(m.view.selected, 10);
        m.update(Msg::PageDown, 0);
        assert_eq!(m.view.selected, 14);
        m.update(Msg::SelectNext, 0);
        assert_eq!(m.view.selected, 14);
        m.update(Msg::PageUp, 0);
        assert_eq!(m.view.selected, 4);
        m.update(Msg::PageUp, 0);
        assert_eq!(m.view.selected, 0);
        m.update(Msg::Select(99), 0);
        assert_eq!(m.view.selected, 0);
        m.update(Msg::Select(3), 0);
        assert_eq!(m.view.selected, 3);
    }

    #[test]
    fn selection_on_empty_results() {
        let mut m = Model::default();
        m.update(Msg::SelectNext, 0);
        m.update(Msg::PageDown, 0);
        assert_eq!(m.view.selected, 0);
        assert!(m.selected_item().is_none());
    }

    #[test]
    fn hide_resets_view() {
        let mut m = loaded(vec![login("a", "GitHub"), login("b", "Bank")]);
        m.update(Msg::Show, 0);
        m.update(Msg::QueryChanged("git".into()), 0);
        m.view.mode = Mode::Detail {
            key: ItemKey::new("s", "a"),
        };
        m.view.revealed = Some(SecretString::from("x"));
        m.view.notice = Some(Notice {
            id: 1,
            text: "n".into(),
        });
        let cancel = CancellationToken::new();
        m.view.pending = Some(PendingFetch {
            key: ItemKey::new("s", "a"),
            cancel: cancel.clone(),
            secret: true,
        });
        let fx = m.update(Msg::Hide, 0);
        assert!(matches!(fx[..], [Effect::HideWindow]));
        assert!(!m.view.visible);
        assert_eq!(m.view.query, "");
        assert_eq!(m.view.mode, Mode::List);
        assert!(m.view.revealed.is_none());
        assert!(m.view.notice.is_none());
        assert!(m.view.pending.is_none());
        assert!(cancel.is_cancelled());
        assert_eq!(m.view.selected, 0);
        assert_eq!(titles(&m).len(), 2, "results show the empty query again");
    }

    #[test]
    fn data_loaded_replaces_items_and_keeps_query() {
        let mut m = loaded(vec![login("a", "GitHub")]);
        m.update(Msg::QueryChanged("bank".into()), 0);
        assert!(titles(&m).is_empty());
        m.data.stale = true;
        m.data.refreshing = true;
        m.update(
            Msg::DataLoaded(listing(vec![login("b", "Bank"), login("c", "Bankline")])),
            2_000,
        );
        assert_eq!(m.data.items.len(), 2);
        assert_eq!(titles(&m), ["Bank", "Bankline"]);
        assert_eq!(m.data.fetched_at, Some(2_000));
        assert!(!m.data.stale);
        assert!(!m.data.refreshing);
        assert_eq!(m.data.source, DataSource::Memory);
        assert_eq!(m.data.vaults.len(), 1);
    }

    #[test]
    fn data_loaded_keeps_selected_item_when_possible() {
        let mut m = loaded(vec![login("a", "Alpha"), login("b", "Beta")]);
        m.update(Msg::SelectNext, 0);
        m.update(
            Msg::DataLoaded(listing(vec![
                login("0", "Aardvark"),
                login("a", "Alpha"),
                login("b", "Beta"),
            ])),
            0,
        );
        assert_eq!(m.selected_item().unwrap().title, "Beta");
    }

    #[test]
    fn only_one_refresh_at_a_time() {
        let mut m = Model::default();
        assert!(matches!(
            m.update(Msg::RefreshRequested, 0)[..],
            [Effect::Refresh]
        ));
        assert!(m.data.refreshing);
        assert!(m.update(Msg::RefreshRequested, 0).is_empty());
    }

    #[test]
    fn refresh_failure_marks_data_stale() {
        let mut m = loaded(vec![login("a", "GitHub")]);
        m.update(Msg::RefreshRequested, 0);
        m.update(Msg::RefreshFailed(PassError::Network), 0);
        assert!(!m.data.refreshing);
        assert!(m.data.stale);
        assert_eq!(m.data.items.len(), 1);
    }

    #[test]
    fn notice_expires_only_if_current() {
        let mut m = Model::default();
        let fx = m.notify("first");
        assert!(matches!(fx, Effect::ExpireNotice { id: 1, .. }));
        m.notify("second");
        m.update(Msg::NoticeExpired(1), 0);
        assert_eq!(m.view.notice.as_ref().unwrap().text, "second");
        m.update(Msg::NoticeExpired(2), 0);
        assert!(m.view.notice.is_none());
    }

    fn account(id: &str) -> AccountId {
        AccountId(id.into())
    }

    fn names(fx: &[Effect]) -> Vec<&'static str> {
        fx.iter()
            .map(|e| match e {
                Effect::Refresh => "Refresh",
                Effect::ProbeSession => "ProbeSession",
                Effect::DeleteCache => "DeleteCache",
                Effect::StartLogin => "StartLogin",
                Effect::LoadCache => "LoadCache",
                Effect::Persist => "Persist",
                _ => "other",
            })
            .collect()
    }

    #[test]
    fn startup_probes_session() {
        let mut m = Model::default();
        let fx = m.update(Msg::Startup, 0);
        assert_eq!(names(&fx), ["LoadCache", "ProbeSession"]);
        assert_eq!(m.session, SessionState::Checking);
    }

    #[test]
    fn probe_success_signs_in_and_refreshes() {
        let mut m = Model::default();
        m.update(Msg::Startup, 0);
        let fx = m.update(Msg::SessionProbed(Ok(account("a"))), 0);
        assert_eq!(m.session, SessionState::SignedIn(account("a")));
        assert_eq!(names(&fx), ["Refresh"]);
    }

    #[test]
    fn probe_errors_map_to_states() {
        for (err, state) in [
            (PassError::SignedOut, SessionState::SignedOut),
            (PassError::Locked, SessionState::Locked),
            (PassError::CliMissing, SessionState::CliMissing),
            (
                PassError::Cli {
                    message: "boom".into(),
                },
                SessionState::Error("pass-cli failed: boom".into()),
            ),
        ] {
            let mut m = Model::default();
            m.update(Msg::SessionProbed(Err(err.clone())), 0);
            assert_eq!(m.session, state, "{err:?}");
        }
    }

    #[test]
    fn signed_out_deletes_cache_and_clears_items() {
        let mut m = loaded(vec![login("a", "A")]);
        m.session = SessionState::SignedIn(account("a"));
        let fx = m.update(Msg::RefreshFailed(PassError::SignedOut), 0);
        assert_eq!(m.session, SessionState::SignedOut);
        assert!(names(&fx).contains(&"DeleteCache"));
        assert!(m.data.items.is_empty());
    }

    #[test]
    fn network_error_keeps_session() {
        let mut m = loaded(vec![login("a", "A")]);
        m.session = SessionState::SignedIn(account("a"));
        m.update(Msg::SessionProbed(Err(PassError::Network)), 0);
        assert_eq!(m.session, SessionState::SignedIn(account("a")));
        assert!(m.data.stale);
        m.update(Msg::RefreshFailed(PassError::Timeout), 0);
        assert_eq!(m.session, SessionState::SignedIn(account("a")));
    }

    #[test]
    fn other_account_deletes_cache_and_refreshes() {
        let mut m = loaded(vec![login("a", "A")]);
        m.session = SessionState::SignedIn(account("a"));
        let fx = m.update(Msg::SessionProbed(Ok(account("b"))), 0);
        assert_eq!(names(&fx), ["DeleteCache", "Refresh"]);
        assert!(m.data.items.is_empty());
        assert_eq!(m.session, SessionState::SignedIn(account("b")));
    }

    #[test]
    fn login_flow() {
        let mut m = Model::default();
        m.update(Msg::SessionProbed(Err(PassError::SignedOut)), 0);
        assert!(m.can_start_login());
        let fx = m.update(Msg::StartLogin, 0);
        assert_eq!(names(&fx), ["StartLogin"]);
        assert_eq!(m.session, SessionState::LoggingIn);
        assert!(!m.can_start_login());
        assert!(m.update(Msg::StartLogin, 0).is_empty(), "no second login");
        m.update(Msg::LoginLine("visit https://x.example/a b".into()), 0);
        assert_eq!(m.login_url.as_deref(), Some("https://x.example/a"));
        m.update(Msg::LoginLine("https://y.example".into()), 0);
        assert_eq!(
            m.login_url.as_deref(),
            Some("https://x.example/a"),
            "first URL wins"
        );
        let fx = m.update(Msg::LoginFinished(Ok(())), 0);
        assert_eq!(m.session, SessionState::Checking);
        assert_eq!(names(&fx), ["ProbeSession"]);
        assert!(m.login_url.is_none());
    }

    #[test]
    fn start_login_ignored_when_signed_in() {
        let mut m = Model {
            session: SessionState::SignedIn(account("a")),
            ..Model::default()
        };
        assert!(m.update(Msg::StartLogin, 0).is_empty());
    }

    fn prefs_mode(m: &Model) -> Option<Option<Action>> {
        match m.view.mode {
            Mode::Preferences { rebinding } => Some(rebinding),
            _ => None,
        }
    }

    fn saved(fx: &[Effect]) -> Option<&Preferences> {
        fx.iter().find_map(|e| match e {
            Effect::SavePreferences(p) => Some(p),
            _ => None,
        })
    }

    #[test]
    fn preferences_open_and_close() {
        let mut m = Model::default();
        m.update(Msg::Show, 0);
        m.update(Msg::OpenPreferences, 0);
        assert_eq!(prefs_mode(&m), Some(None));
        m.update(Msg::Escape, 0);
        assert_eq!(m.view.mode, Mode::List);
        assert!(m.view.visible);
    }

    #[test]
    fn clipboard_timeout_is_adjusted_and_clamped() {
        let mut m = Model::default();
        m.update(Msg::OpenPreferences, 0);
        let fx = m.update(Msg::AdjustClipboardClear(30), 0);
        assert_eq!(m.prefs.clipboard_clear_secs, 120);
        assert_eq!(saved(&fx).map(|p| p.clipboard_clear_secs), Some(120));
        m.update(Msg::AdjustClipboardClear(-1000), 0);
        assert_eq!(m.prefs.clipboard_clear_secs, 10);
        assert!(
            m.update(Msg::AdjustClipboardClear(-10), 0).is_empty(),
            "no change, no save"
        );
    }

    #[test]
    fn rebinding_a_shortcut() {
        use crate::config::Modifier;
        let mut m = Model::default();
        m.update(Msg::OpenPreferences, 0);
        m.update(Msg::StartRebind(Action::CopyTotp), 0);
        assert_eq!(prefs_mode(&m), Some(Some(Action::CopyTotp)));
        let chord = KeyChord::new(&[Modifier::Ctrl], "t");
        let fx = m.update(Msg::ChordCaptured(chord.clone()), 0);
        assert_eq!(m.prefs.chord(Action::CopyTotp), chord);
        assert!(saved(&fx).is_some());
        assert_eq!(prefs_mode(&m), Some(None));
    }

    #[test]
    fn duplicate_chord_is_rejected() {
        use crate::config::Modifier;
        let mut m = Model::default();
        m.update(Msg::OpenPreferences, 0);
        m.update(Msg::StartRebind(Action::CopyTotp), 0);
        let fx = m.update(Msg::ChordCaptured(KeyChord::new(&[Modifier::Ctrl], "u")), 0);
        assert!(saved(&fx).is_none());
        assert!(
            m.view
                .notice
                .as_ref()
                .unwrap()
                .text
                .contains("Copy username")
        );
        assert_eq!(
            prefs_mode(&m),
            Some(Some(Action::CopyTotp)),
            "still waiting"
        );
        m.update(Msg::Escape, 0);
        assert_eq!(prefs_mode(&m), Some(None), "escape cancels rebinding first");
    }

    #[test]
    fn chord_outside_rebinding_is_ignored() {
        let mut m = Model::default();
        assert!(
            m.update(Msg::ChordCaptured(KeyChord::new(&[], "x")), 0)
                .is_empty()
        );
    }

    #[test]
    fn reset_and_external_load() {
        use crate::config::Modifier;
        let mut m = Model::default();
        m.update(Msg::OpenPreferences, 0);
        m.update(Msg::StartRebind(Action::Refresh), 0);
        m.update(Msg::ChordCaptured(KeyChord::new(&[Modifier::Alt], "r")), 0);
        let fx = m.update(Msg::ResetShortcuts, 0);
        assert_eq!(m.prefs.shortcuts, Preferences::default().shortcuts);
        assert!(saved(&fx).is_some());

        let mut m = loaded((0..30).map(|i| login(&i.to_string(), "x")).collect());
        m.update(
            Msg::PrefsLoaded(Preferences {
                max_results: 10,
                clipboard_clear_secs: 5,
                ..Preferences::default()
            }),
            0,
        );
        assert_eq!(m.view.results.len(), 10);
        assert_eq!(m.prefs.clipboard_clear_secs, 10, "validated on load");
    }

    #[test]
    fn max_results_comes_from_preferences() {
        let mut m = Model::new(Preferences {
            max_results: 10,
            ..Preferences::default()
        });
        m.update(Msg::DataLoaded(listing(many(30))), 0);
        assert_eq!(m.view.results.len(), 10);
    }
}
