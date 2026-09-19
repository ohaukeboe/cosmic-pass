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
use crate::model::{
    AccountId, CACHE_FORMAT_VERSION, CacheFile, FieldRef, ItemKey, ItemSummary, Vault,
};
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

/// Why the last refresh failed, so the status line can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshError {
    /// Proton Pass could not be reached: the network is down or `pass-cli` timed out.
    Unreachable,
    /// Any other failure; the shown data is simply out of date.
    Other,
}

impl RefreshError {
    fn of(error: &PassError) -> Self {
        match error {
            PassError::Network | PassError::Timeout => Self::Unreachable,
            _ => Self::Other,
        }
    }
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
    /// Why the last refresh failed, cleared by the next successful one.
    pub last_error: Option<RefreshError>,
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

/// The secret the detail pane reveals, or is waiting on. A bare position is not enough to
/// identify it: a later listing may delete, rename or reorder fields, and the plaintext would
/// then be drawn under whatever field slid into that slot. Pinning the item, the position and
/// the field as it stood when the fetch was asked for lets the reveal be dropped the moment it
/// stops describing what is on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealTarget {
    /// Identifies the fetch issued for this target. An item's positions are reused as it is
    /// reshaped and a fetch already resolved cannot be cancelled, so only a number that is
    /// never issued twice can keep one reveal's value off another reveal's field.
    generation: u64,
    pub key: ItemKey,
    /// Position in the item's fields. Field names repeat, positions do not.
    pub index: usize,
    /// The field as it was at fetch time. Names alone do not separate two secrets sharing
    /// one, so the label rides along.
    field: FieldRef,
    /// The item's edit time at fetch time: a later edit may have rotated this very secret.
    modified_at: i64,
}

impl RevealTarget {
    /// Whether `item` still carries the same secret field, unedited, at the same position.
    fn still_describes(&self, item: &ItemSummary) -> bool {
        item.key == self.key
            && item.modified_at == self.modified_at
            && item.fields.get(self.index) == Some(&self.field)
    }
}

#[derive(Debug, Default)]
pub struct ViewState {
    pub visible: bool,
    pub query: String,
    pub results: Vec<ResultRow>,
    pub selected: usize,
    pub mode: Mode,
    pub pending: Option<PendingFetch>,
    /// The plain value of the secret field on show in the detail pane, once fetched.
    pub revealed: Option<SecretString>,
    /// What that value belongs to, and what the pane is waiting on while a fetch is in
    /// flight. Also the position the reveal cycle has reached, so a failed fetch does not
    /// send the next press back to the first secret.
    pub revealed_field: Option<RevealTarget>,
    pub totp: Option<TotpDisplay>,
    /// A detail-pane fetch (TOTP or reveal) is in flight; cancelled when leaving the pane.
    pub detail_cancel: Option<CancellationToken>,
    /// Cancels just the reveal fetch, so re-masking cannot stop the one-time code fetch
    /// sharing `detail_cancel`.
    pub reveal_cancel: Option<CancellationToken>,
    pub totp_fetching: bool,
    pub notice: Option<Notice>,
    /// A copy is waiting for the clipboard helper to take ownership of the selection.
    pub copying: bool,
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
        /// Index into the item's fields of the revealed field.
        index: usize,
        /// The `generation` of the `FetchReveal` this answers.
        generation: u64,
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
    /// The account the loaded data belongs to: whoever was signed in when the listing
    /// arrived, or the account the cache file names. An account switch is measured against
    /// this, not against `session`, which may have passed through `Locked` since.
    data_account: Option<AccountId>,
    index: SearchIndex,
    next_notice: u64,
    next_reveal: u64,
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

    /// The status line to show while the shown data may be out of date, if any.
    pub fn stale_notice(&self) -> Option<&'static str> {
        if !self.data.stale || self.data.refreshing {
            return None;
        }
        Some(match self.data.last_error {
            Some(RefreshError::Unreachable) if self.data.items.is_empty() => {
                "Can’t reach Proton Pass. Press F5 to retry."
            }
            Some(RefreshError::Unreachable) => {
                "Can’t reach Proton Pass — showing saved items. Press F5 to retry."
            }
            Some(RefreshError::Other) | None => "Data may be out of date. Press F5 to refresh.",
        })
    }

    /// The account currently signed in, if the session knows one.
    fn signed_in_account(&self) -> Option<AccountId> {
        match &self.session {
            SessionState::SignedIn(account) => Some(account.clone()),
            _ => None,
        }
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
                // An unclassified failure says nothing about whether a retry would fare
                // better, so refreshing behind the error panel just repeats it on every
                // window opening. The panel's Try again re-probes instead.
                | SessionState::Error(_)
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

    /// The item's secret fields as `(index, name)`, in the order the detail pane renders
    /// them. Bind by position: several fields may share a name.
    fn secret_fields(&self, key: &ItemKey) -> Vec<(usize, String)> {
        self.item(key).map_or_else(Vec::new, |item| {
            item.fields
                .iter()
                .enumerate()
                .filter(|(_, f)| f.secret)
                .map(|(i, f)| (i, f.name.clone()))
                .collect()
        })
    }

    /// Pins the field at `index` of `key` to the item as it stands now, so a later listing
    /// can be checked against it.
    fn reveal_target(&self, key: &ItemKey, index: usize, generation: u64) -> Option<RevealTarget> {
        let item = self.item(key)?;
        Some(RevealTarget {
            generation,
            key: key.clone(),
            index,
            field: item.fields.get(index)?.clone(),
            modified_at: item.modified_at,
        })
    }

    /// The plain value of the field at `index`, if that is the secret the pane reveals. The
    /// target is re-checked against the item on show, so a plaintext can never be drawn
    /// beside a label it was not fetched for even if an invalidation were missed upstream.
    pub fn revealed_value(&self, index: usize) -> Option<&SecretString> {
        let target = self.view.revealed_field.as_ref()?;
        let value = self.view.revealed.as_ref()?;
        let item = self.target_item()?;
        (target.index == index && target.still_describes(item)).then_some(value)
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
                self.data.last_error = None;
                self.data.source = DataSource::Memory;
                self.data.fetched_at = Some(now);
                self.data.cached_at = None;
                // A listing can land after the session stopped naming an account — a refresh
                // that raced a lock or a network error. Keep whose items these are rather
                // than forgetting, or `signed_in` cannot tell a later switch from a return.
                if let Some(account) = self.signed_in_account() {
                    self.data_account = Some(account);
                }
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
            Msg::RevealFetched {
                key,
                index,
                generation,
                result,
            } => {
                // Dropped once the pane has re-masked or moved on to another field, so a late
                // result cannot surface under a label it does not belong to. The generation
                // is what settles it: a fetch that has already resolved cannot be cancelled,
                // and the field it was asked for may since have been replaced by another at
                // the same position, which the pinned target would describe just as well.
                let awaited = self
                    .view
                    .revealed_field
                    .as_ref()
                    .is_some_and(|t| t.generation == generation && t.index == index);
                if !self.in_detail(&key) || !awaited {
                    return vec![];
                }
                self.view.reveal_cancel = None;
                match result {
                    Ok(value) => {
                        self.view.revealed = Some(value);
                        vec![]
                    }
                    Err(PassError::Cancelled) => vec![],
                    Err(e) => {
                        // The target stays put, masked: it marks how far the cycle has come, so
                        // the next press moves on to the field after the failing one instead of
                        // restarting and failing here forever.
                        vec![self.notify(e.to_string())]
                    }
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
            Msg::CopyFinished(Ok(())) => self.copy_confirmed(),
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
        self.data_account = Some(file.account);
        self.usage = UsageTable::from_records(file.usage);
        self.replace_items(crate::pass::backend::Listing {
            vaults: file.vaults,
            items: file.items,
        });
        vec![]
    }

    fn signed_in(&mut self, account: AccountId) -> Vec<Effect> {
        let mut effects = Vec::new();
        // Whose data is on screen is what settles this. `session` alone cannot: one that went
        // through `Locked`, `Error` or `Checking` no longer names the previous account, and
        // that account's items and revealed secrets would survive the switch.
        let other_session =
            matches!(&self.session, SessionState::SignedIn(previous) if *previous != account);
        let other_data = self.data_account.as_ref().is_some_and(|a| *a != account);
        if other_session || other_data {
            self.clear_data();
            effects.push(Effect::DeleteCache);
        }
        self.session = SessionState::SignedIn(account.clone());
        self.data_account = Some(account);
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
            // Every call fails until the user resets pass-cli's store, so say so in the panel
            // rather than leaving a "may be out of date" line over items that never arrive.
            PassError::LocalData => {
                self.session = SessionState::Error(PassError::LocalData.to_string());
                vec![]
            }
            other => {
                self.data.stale = true;
                self.data.last_error = Some(RefreshError::of(&other));
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
        self.data_account = None;
        self.index = SearchIndex::default();
        // Nothing fetched under the account being dropped may stay on screen (FR-025).
        self.clear_detail_secrets();
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
        self.clear_detail_secrets();
    }

    /// Forgets every secret the detail pane holds and stops the fetches feeding it.
    fn clear_detail_secrets(&mut self) {
        self.drop_reveal();
        self.view.totp = None;
        self.view.totp_fetching = false;
        if let Some(cancel) = self.view.detail_cancel.take() {
            cancel.cancel();
        }
    }

    /// Forgets the revealed secret and stops the fetch still running for it, if any.
    fn drop_reveal(&mut self) {
        self.view.revealed = None;
        self.view.revealed_field = None;
        self.cancel_reveal();
    }

    /// Stops the reveal fetch in flight, if any. The reveal has its own token: cancelling
    /// `detail_cancel` would take the one-time code fetch down with it (FR-019).
    fn cancel_reveal(&mut self) {
        if let Some(cancel) = self.view.reveal_cancel.take() {
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

    /// Reveals the next secret field of the detail item. Every secret field is revealable,
    /// one at a time, and the press after the last one re-masks. Only one fetch may be
    /// outstanding: a press while one is in flight cancels it and re-masks rather than
    /// moving the pane on to another field.
    fn toggle_reveal(&mut self) -> Vec<Effect> {
        let Mode::Detail { key } = &self.view.mode else {
            return vec![];
        };
        let key = key.clone();
        // A reveal fetch holds its token for exactly as long as it is in flight.
        let in_flight = self.view.reveal_cancel.is_some();
        let reached = self.view.revealed_field.take();
        self.view.revealed = None;
        self.cancel_reveal();
        if in_flight {
            return vec![];
        }
        let secrets = self.secret_fields(&key);
        let next = match reached {
            Some(target) => secrets
                .iter()
                .position(|(i, _)| *i == target.index)
                .map_or(0, |at| at + 1),
            None => 0,
        };
        let Some((index, field)) = secrets.into_iter().nth(next) else {
            return vec![];
        };
        self.next_reveal += 1;
        let generation = self.next_reveal;
        let Some(target) = self.reveal_target(&key, index, generation) else {
            return vec![];
        };
        self.view.revealed_field = Some(target);
        let cancel = CancellationToken::new();
        self.view.reveal_cancel = Some(cancel.clone());
        vec![Effect::FetchReveal {
            key,
            field,
            index,
            generation,
            cancel,
        }]
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

    /// Records usage for a copy the clipboard helper has yet to confirm.
    fn finish_copy(&mut self, key: &ItemKey, now: i64) -> Vec<Effect> {
        self.usage.record(key, now);
        self.view.copying = true;
        vec![Effect::Persist]
    }

    /// Hides the window once the helper owns the selection, so a paste right
    /// after Enter cannot read an empty one (SC-003). A confirmation that
    /// arrives after the window was dismissed must not disturb a fresh session.
    fn copy_confirmed(&mut self) -> Vec<Effect> {
        if !self.view.copying {
            return vec![];
        }
        self.hide()
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
        self.drop_stale_reveal();
    }

    /// Drops the revealed secret, and any fetch still running for it, once the item it was
    /// taken from no longer carries that field at that position. Refreshes are routine — the
    /// pane refreshes as it opens — so an item that came back unchanged keeps its reveal.
    fn drop_stale_reveal(&mut self) {
        let intact = match &self.view.revealed_field {
            Some(target) => self
                .item(&target.key)
                .is_some_and(|item| target.still_describes(item)),
            None => return,
        };
        if !intact {
            self.drop_reveal();
        }
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
    use crate::model::{FieldRef, ItemKind, ShareId};
    use secrecy::ExposeSecret;

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
        // Independent of the default result cap.
        let mut m = Model::new(Preferences {
            max_results: 50,
            ..Preferences::default()
        });
        m.update(Msg::DataLoaded(listing(many(15))), 1_000);
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
    fn unreachable_refresh_is_reported_as_such() {
        for error in [PassError::Network, PassError::Timeout] {
            let mut m = loaded(vec![login("a", "GitHub")]);
            m.update(Msg::RefreshRequested, 0);
            m.update(Msg::RefreshFailed(error.clone()), 0);
            assert_eq!(
                m.data.last_error,
                Some(RefreshError::Unreachable),
                "{error}"
            );
            let notice = m.stale_notice().expect("a status line");
            assert!(notice.contains("reach Proton Pass"), "{notice}");
        }
    }

    #[test]
    fn an_unreachable_refresh_with_nothing_cached_promises_no_saved_items() {
        let mut m = Model::default();
        m.update(Msg::RefreshRequested, 0);
        m.update(Msg::RefreshFailed(PassError::Network), 0);
        assert!(m.data.items.is_empty());
        let notice = m.stale_notice().expect("a status line");
        assert!(notice.contains("reach Proton Pass"), "{notice}");
        assert!(!notice.contains("saved items"), "{notice}");
    }

    #[test]
    fn other_refresh_failures_keep_the_generic_status_line() {
        let mut m = loaded(vec![login("a", "GitHub")]);
        m.update(Msg::RefreshRequested, 0);
        m.update(Msg::RefreshFailed(PassError::NotFound), 0);
        assert_eq!(m.data.last_error, Some(RefreshError::Other));
        let notice = m.stale_notice().expect("a status line");
        assert!(notice.contains("out of date"), "{notice}");
    }

    #[test]
    fn a_successful_refresh_clears_the_failure() {
        let mut m = loaded(vec![login("a", "GitHub")]);
        m.update(Msg::RefreshRequested, 0);
        m.update(Msg::RefreshFailed(PassError::Network), 0);
        m.update(Msg::DataLoaded(listing(vec![login("a", "GitHub")])), 2_000);
        assert_eq!(m.data.last_error, None);
        assert_eq!(m.stale_notice(), None);
    }

    #[test]
    fn no_status_line_while_refreshing_again() {
        let mut m = loaded(vec![login("a", "GitHub")]);
        m.update(Msg::RefreshRequested, 0);
        m.update(Msg::RefreshFailed(PassError::Network), 0);
        m.update(Msg::RefreshRequested, 0);
        assert!(m.data.refreshing);
        assert_eq!(m.stale_notice(), None);
    }

    #[test]
    fn cached_data_without_a_failure_is_merely_stale() {
        let mut m = Model::default();
        m.update(
            Msg::CacheLoaded(Some(CacheFile {
                format_version: CACHE_FORMAT_VERSION,
                account: AccountId("account-1".into()),
                fetched_at: 10,
                vaults: listing(vec![]).vaults,
                items: vec![login("a", "GitHub")],
                usage: vec![],
            })),
            1_000,
        );
        assert!(m.data.stale);
        assert_eq!(m.data.last_error, None);
        let notice = m.stale_notice().expect("a status line");
        assert!(notice.contains("out of date"), "{notice}");
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

    fn with_url(id: &str, title: &str) -> ItemSummary {
        ItemSummary {
            fields: vec![FieldRef::plain(
                "url",
                "Website",
                "https://example.invalid".into(),
            )],
            ..login(id, title)
        }
    }

    fn with_password(id: &str, title: &str) -> ItemSummary {
        ItemSummary {
            fields: vec![FieldRef::secret("password", "Password")],
            ..login(id, title)
        }
    }

    fn opened(items: Vec<ItemSummary>) -> Model {
        let mut m = loaded(items);
        m.update(Msg::Show, 0);
        m
    }

    #[test]
    fn a_stored_copy_hides_only_once_the_clipboard_confirms() {
        let mut m = opened(vec![with_url("a", "GitHub")]);
        let fx = m.update(Msg::CopyUrl, 5_000);
        assert!(matches!(
            fx[..],
            [Effect::Copy { secret: false, .. }, Effect::Persist]
        ));
        assert!(m.view.visible, "the window stays up until the copy lands");
        assert_eq!(m.usage.recency(&ItemKey::new("s", "a")), Some(5_000));
        let fx = m.update(Msg::CopyFinished(Ok(())), 5_000);
        assert!(matches!(fx[..], [Effect::HideWindow]));
        assert!(!m.view.visible);
    }

    #[test]
    fn a_fetched_copy_hides_only_once_the_clipboard_confirms() {
        let mut m = opened(vec![with_password("a", "GitHub")]);
        let fx = m.update(Msg::CopyPrimary, 0);
        assert!(matches!(
            fx[..],
            [Effect::FetchAndCopy { secret: true, .. }]
        ));
        let fx = m.update(
            Msg::CopyFetched {
                key: ItemKey::new("s", "a"),
                result: Ok(SecretString::from("hunter2")),
            },
            5_000,
        );
        assert!(matches!(
            fx[..],
            [Effect::Copy { secret: true, .. }, Effect::Persist]
        ));
        assert!(m.view.visible);
        assert!(m.view.pending.is_none());
        let fx = m.update(Msg::CopyFinished(Ok(())), 5_000);
        assert!(matches!(fx[..], [Effect::HideWindow]));
    }

    #[test]
    fn a_clipboard_failure_keeps_the_window_up() {
        let mut m = opened(vec![with_url("a", "GitHub")]);
        m.update(Msg::CopyUrl, 0);
        let fx = m.update(Msg::CopyFinished(Err("no clipboard".into())), 0);
        assert!(matches!(fx[..], [Effect::ExpireNotice { .. }]));
        assert_eq!(
            m.view.notice.as_ref().map(|n| n.text.as_str()),
            Some("Clipboard unavailable")
        );
        assert!(m.view.visible);
    }

    #[test]
    fn a_confirmation_after_the_window_closed_is_ignored() {
        let mut m = opened(vec![with_url("a", "GitHub")]);
        m.update(Msg::CopyUrl, 0);
        m.update(Msg::Escape, 0);
        m.update(Msg::Show, 0);
        m.update(Msg::QueryChanged("git".into()), 0);
        assert!(m.update(Msg::CopyFinished(Ok(())), 0).is_empty());
        assert!(m.view.visible);
        assert_eq!(m.view.query, "git");
    }

    #[test]
    fn a_vanished_item_keeps_the_window_up_for_the_notice() {
        let mut m = opened(vec![with_password("a", "GitHub")]);
        m.update(Msg::CopyPrimary, 0);
        let fx = m.update(
            Msg::CopyFetched {
                key: ItemKey::new("s", "a"),
                result: Err(PassError::NotFound),
            },
            0,
        );
        assert_eq!(names(&fx), ["other", "Refresh"]);
        assert_eq!(
            m.view.notice.as_ref().map(|n| n.text.as_str()),
            Some("Item no longer exists")
        );
        assert!(m.view.visible);
        assert!(!m.view.copying);
    }

    /// A card carries several secret fields, and a plain one the reveal cycle must skip.
    fn card(id: &str) -> ItemSummary {
        ItemSummary {
            kind: ItemKind::CreditCard,
            fields: vec![
                FieldRef::plain("cardholder", "Cardholder", "Ada Lovelace".into()),
                FieldRef::secret("number", "Card number"),
                FieldRef::secret("cvv", "Verification number"),
                FieldRef::secret("pin", "PIN"),
            ],
            ..login(id, "Visa")
        }
    }

    fn detail(items: Vec<ItemSummary>) -> Model {
        let mut m = opened(items);
        m.update(Msg::OpenDetail, 0);
        m
    }

    /// The `(field index, name)` a single `FetchReveal` asks for.
    fn revealing(fx: &[Effect]) -> Option<(usize, &str)> {
        match fx {
            [Effect::FetchReveal { field, index, .. }] => Some((*index, field.as_str())),
            _ => None,
        }
    }

    /// The identity stamped on a single `FetchReveal`, so the result of that very fetch can
    /// be delivered after the pane has moved on.
    fn reveal_id(fx: &[Effect]) -> u64 {
        match fx {
            [Effect::FetchReveal { generation, .. }] => *generation,
            _ => 0,
        }
    }

    /// The fetch the pane is waiting on; `0` is never issued.
    fn awaited_reveal(m: &Model) -> u64 {
        m.view.revealed_field.as_ref().map_or(0, |t| t.generation)
    }

    fn reveal_fetched_for(m: &mut Model, id: &str, generation: u64, index: usize, value: &str) {
        m.update(
            Msg::RevealFetched {
                key: ItemKey::new("s", id),
                index,
                generation,
                result: Ok(SecretString::from(value)),
            },
            0,
        );
    }

    /// Delivers the value the pane is waiting for.
    fn reveal_fetched(m: &mut Model, id: &str, index: usize, value: &str) {
        reveal_fetched_for(m, id, awaited_reveal(m), index, value);
    }

    #[test]
    fn reveal_walks_every_secret_field_and_then_re_masks() {
        let mut m = detail(vec![card("a")]);
        for (index, name) in [(1, "number"), (2, "cvv"), (3, "pin")] {
            let fx = m.update(Msg::ToggleReveal, 0);
            assert_eq!(
                revealing(&fx),
                Some((index, name)),
                "the plain field is skipped"
            );
            reveal_fetched(&mut m, "a", index, name);
            assert_eq!(
                m.revealed_value(index).map(|v| v.expose_secret()),
                Some(name)
            );
        }
        let fx = m.update(Msg::ToggleReveal, 0);
        assert!(fx.is_empty(), "past the last secret nothing is fetched");
        assert!(m.view.revealed.is_none());
        assert!(m.view.revealed_field.is_none());
    }

    #[test]
    fn only_the_field_that_asked_for_a_value_shows_it() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        assert_eq!(m.revealed_value(1).map(|v| v.expose_secret()), Some("4111"));
        assert!(m.revealed_value(2).is_none());
        assert!(m.revealed_value(3).is_none());
    }

    #[test]
    fn a_reveal_result_that_arrives_after_re_masking_is_dropped() {
        let mut m = detail(vec![card("a")]);
        let fx = m.update(Msg::ToggleReveal, 0);
        let abandoned = reveal_id(&fx);
        let fx = m.update(Msg::ToggleReveal, 0);
        assert!(
            fx.is_empty(),
            "a press mid-fetch re-masks instead of moving on"
        );
        reveal_fetched_for(&mut m, "a", abandoned, 1, "4111");
        assert!(m.view.revealed.is_none());
        assert!(m.revealed_value(1).is_none());
    }

    #[test]
    fn leaving_the_pane_drops_a_revealed_secret() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        m.update(Msg::Back, 0);
        assert!(m.view.revealed.is_none());
        assert!(m.view.revealed_field.is_none());
    }

    /// Two sections of one item can each carry a secret of the same name.
    fn twin_pins(id: &str) -> ItemSummary {
        ItemSummary {
            fields: vec![
                FieldRef::secret("pin", "PIN"),
                FieldRef::secret("pin", "Backup PIN"),
            ],
            ..login(id, "Bank")
        }
    }

    fn totp_card(id: &str) -> ItemSummary {
        ItemSummary {
            totp_fields: vec!["totp_uri".into()],
            ..card(id)
        }
    }

    #[test]
    fn a_reveal_result_for_a_field_the_pane_moved_on_from_is_dropped() {
        let mut m = detail(vec![card("a")]);
        let fx = m.update(Msg::ToggleReveal, 0);
        let abandoned = reveal_id(&fx);
        // A press mid-fetch re-masks, leaving the first fetch to land late.
        m.update(Msg::ToggleReveal, 0);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(revealing(&fx), Some((2, "cvv")));
        reveal_fetched_for(&mut m, "a", abandoned, 1, "4111");
        assert!(m.view.revealed.is_none(), "the late result is dropped");
        assert!(m.revealed_value(2).is_none());
    }

    #[test]
    fn a_late_reveal_result_cannot_surface_under_the_field_that_took_its_place() {
        let plain = FieldRef::plain("username", "Username", "me".into());
        let mut m = detail(vec![github(vec![
            plain.clone(),
            FieldRef::secret("password", "Password"),
            FieldRef::secret("recovery", "Recovery code"),
        ])]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "hunter2");
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(revealing(&fx), Some((2, "recovery")));
        // The fetch for the recovery code is under way when a listing puts a PIN in its
        // place; the value is already resolved, so cancelling can no longer stop it.
        let in_flight = reveal_id(&fx);
        m.update(
            Msg::DataLoaded(listing(vec![github(vec![
                plain,
                FieldRef::secret("password", "Password"),
                FieldRef::secret("pin", "PIN"),
            ])])),
            2_000,
        );
        // The user reveals again and walks down to the PIN, which sits where the recovery
        // code did, in an item the pinned target now describes perfectly.
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "hunter2");
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(revealing(&fx), Some((2, "pin")));
        reveal_fetched_for(&mut m, "a", in_flight, 2, "RECOVERY-CODE");
        assert!(
            m.revealed_value(2).is_none(),
            "the recovery code is not shown as the PIN"
        );
        assert!(m.view.revealed.is_none());
    }

    #[test]
    fn two_secret_fields_sharing_a_name_are_revealed_in_turn() {
        let mut m = detail(vec![twin_pins("a")]);
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(revealing(&fx), Some((0, "pin")));
        reveal_fetched(&mut m, "a", 0, "1111");
        assert!(m.revealed_value(1).is_none(), "the twin stays masked");
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(revealing(&fx), Some((1, "pin")), "the twin is reached too");
        reveal_fetched(&mut m, "a", 1, "2222");
        assert!(m.revealed_value(0).is_none());
        assert_eq!(m.revealed_value(1).map(|v| v.expose_secret()), Some("2222"));
        let fx = m.update(Msg::ToggleReveal, 0);
        assert!(fx.is_empty(), "the press after the last secret re-masks");
        assert!(m.view.revealed.is_none());
        assert!(m.view.revealed_field.is_none());
    }

    #[test]
    fn re_masking_a_reveal_leaves_the_one_time_code_fetch_running() {
        let mut m = opened(vec![totp_card("a")]);
        let fx = m.update(Msg::OpenDetail, 0);
        let [Effect::FetchTotp { cancel: totp, .. }] = &fx[..] else {
            panic!("the pane fetches the one-time code");
        };
        let totp = totp.clone();
        let fx = m.update(Msg::ToggleReveal, 0);
        let [Effect::FetchReveal { cancel: reveal, .. }] = &fx[..] else {
            panic!("the pane fetches the first secret");
        };
        let reveal = reveal.clone();
        m.update(Msg::ToggleReveal, 0);
        assert!(
            reveal.is_cancelled(),
            "the abandoned reveal fetch is cancelled"
        );
        assert!(
            !totp.is_cancelled(),
            "the live one-time code fetch survives"
        );
    }

    /// A login whose secret fields sit below a plain one, as the proven sequence has them.
    fn github(fields: Vec<FieldRef>) -> ItemSummary {
        ItemSummary {
            fields,
            ..login("a", "GitHub")
        }
    }

    fn reveal_failed(m: &mut Model, id: &str, index: usize) -> Vec<Effect> {
        let generation = awaited_reveal(m);
        m.update(
            Msg::RevealFetched {
                key: ItemKey::new("s", id),
                index,
                generation,
                result: Err(PassError::FieldMissing),
            },
            0,
        )
    }

    #[test]
    fn a_refresh_that_swaps_a_secret_field_drops_the_revealed_secret() {
        let plain = FieldRef::plain("username", "Username", "me".into());
        let mut m = detail(vec![github(vec![
            plain.clone(),
            FieldRef::secret("password", "Password"),
            FieldRef::secret("recovery", "Recovery code"),
        ])]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "hunter2");
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(revealing(&fx), Some((2, "recovery")));
        reveal_fetched(&mut m, "a", 2, "RECOVERY-CODE");
        assert_eq!(
            m.revealed_value(2).map(|v| v.expose_secret()),
            Some("RECOVERY-CODE")
        );
        // The recovery code is deleted upstream and a PIN takes the position it held.
        m.update(
            Msg::DataLoaded(listing(vec![github(vec![
                plain,
                FieldRef::secret("password", "Password"),
                FieldRef::secret("pin", "PIN"),
            ])])),
            2_000,
        );
        assert!(
            m.revealed_value(2).is_none(),
            "the recovery code is not re-labelled as the PIN"
        );
        assert!(m.view.revealed.is_none());
        assert!(m.view.revealed_field.is_none());
    }

    #[test]
    fn a_refresh_that_shifts_a_secret_field_drops_the_revealed_secret() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        assert_eq!(m.revealed_value(1).map(|v| v.expose_secret()), Some("4111"));
        // The cardholder line goes away and every field below it moves up one.
        let shifted = ItemSummary {
            fields: card("a").fields[1..].to_vec(),
            ..card("a")
        };
        m.update(Msg::DataLoaded(listing(vec![shifted])), 2_000);
        assert!(
            m.revealed_value(1).is_none(),
            "the card number is not re-labelled as the verification number"
        );
        assert!(m.view.revealed.is_none());
        assert!(m.view.revealed_field.is_none());
    }

    #[test]
    fn a_refresh_that_leaves_the_item_alone_keeps_the_revealed_secret() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        m.update(Msg::DataLoaded(listing(vec![card("a")])), 2_000);
        assert_eq!(
            m.revealed_value(1).map(|v| v.expose_secret()),
            Some("4111"),
            "a refresh that changed nothing does not re-mask the pane"
        );
        assert_eq!(
            m.view.mode,
            Mode::Detail {
                key: ItemKey::new("s", "a")
            }
        );
    }

    #[test]
    fn a_reveal_fetch_survives_a_refresh_that_leaves_the_item_alone() {
        let mut m = detail(vec![card("a")]);
        let fx = m.update(Msg::ToggleReveal, 0);
        let [Effect::FetchReveal { cancel, .. }] = &fx[..] else {
            panic!("the pane fetches the first secret");
        };
        let cancel = cancel.clone();
        m.update(Msg::DataLoaded(listing(vec![card("a")])), 2_000);
        assert!(!cancel.is_cancelled());
        reveal_fetched(&mut m, "a", 1, "4111");
        assert_eq!(m.revealed_value(1).map(|v| v.expose_secret()), Some("4111"));
    }

    #[test]
    fn a_refresh_that_reshapes_the_item_drops_a_reveal_fetch_in_flight() {
        let mut m = detail(vec![card("a")]);
        let fx = m.update(Msg::ToggleReveal, 0);
        let abandoned = reveal_id(&fx);
        let [Effect::FetchReveal { cancel, .. }] = &fx[..] else {
            panic!("the pane fetches the first secret");
        };
        let cancel = cancel.clone();
        let shifted = ItemSummary {
            fields: card("a").fields[1..].to_vec(),
            ..card("a")
        };
        m.update(Msg::DataLoaded(listing(vec![shifted])), 2_000);
        assert!(cancel.is_cancelled());
        assert!(m.view.revealed_field.is_none());
        reveal_fetched_for(&mut m, "a", abandoned, 1, "4111");
        assert!(
            m.view.revealed.is_none(),
            "the result of the abandoned fetch is dropped"
        );
    }

    #[test]
    fn signing_out_clears_a_revealed_secret() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        let fx = m.update(Msg::RefreshFailed(PassError::SignedOut), 0);
        assert_eq!(names(&fx), ["DeleteCache"]);
        assert_eq!(m.session, SessionState::SignedOut);
        assert!(m.view.revealed.is_none());
        assert!(m.view.revealed_field.is_none());
        assert!(m.revealed_value(1).is_none());
    }

    #[test]
    fn switching_account_clears_a_revealed_secret() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::SessionProbed(Ok(account("one"))), 0);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        m.update(Msg::SessionProbed(Ok(account("two"))), 0);
        assert!(m.view.revealed.is_none());
        assert!(m.view.revealed_field.is_none());
        assert!(m.revealed_value(1).is_none());
    }

    /// Signs in as "one", loads that account's items and reveals one of its secrets.
    fn signed_in_as_one() -> Model {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::SessionProbed(Ok(account("one"))), 0);
        m.update(Msg::DataLoaded(listing(vec![card("a")])), 0);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        assert_eq!(m.revealed_value(1).map(|v| v.expose_secret()), Some("4111"));
        m
    }

    #[test]
    fn unlocking_a_different_account_clears_the_previous_one() {
        // F5 finds the session locked; the user unlocks another account and presses Retry.
        let mut m = signed_in_as_one();
        m.update(Msg::RefreshFailed(PassError::Locked), 0);
        assert_eq!(m.session, SessionState::Locked);
        m.update(Msg::Startup, 0);
        let fx = m.update(Msg::SessionProbed(Ok(account("two"))), 0);
        assert!(
            names(&fx).contains(&"DeleteCache"),
            "the first account's cache is deleted (FR-024b)"
        );
        assert!(m.data.items.is_empty(), "its items are gone too");
        assert!(
            m.view.revealed.is_none() && m.revealed_value(1).is_none(),
            "its plaintext is not left on screen (FR-025)"
        );
    }

    #[test]
    fn a_listing_that_lands_after_a_lock_still_names_its_account() {
        // A refresh already in flight when the session locked delivers its listing late. It
        // must not erase whose items are on screen, or the next sign-in cannot see a switch.
        let mut m = signed_in_as_one();
        m.update(Msg::RefreshFailed(PassError::Locked), 0);
        m.update(Msg::DataLoaded(listing(vec![card("a")])), 0);
        let fx = m.update(Msg::SessionProbed(Ok(account("two"))), 0);
        assert!(
            names(&fx).contains(&"DeleteCache"),
            "the first account's cache is deleted (FR-024b)"
        );
        assert!(m.data.items.is_empty(), "its items are gone too");
        assert!(
            m.view.revealed.is_none() && m.revealed_value(1).is_none(),
            "its plaintext is not left on screen (FR-025)"
        );
    }

    #[test]
    fn an_account_switch_is_caught_from_every_prior_session_state() {
        for prior in [
            SessionState::Unknown,
            SessionState::Checking,
            SessionState::Locked,
            SessionState::Error("boom".into()),
            SessionState::SignedIn(account("one")),
        ] {
            let mut m = signed_in_as_one();
            m.session = prior.clone();
            let fx = m.update(Msg::SessionProbed(Ok(account("two"))), 0);
            assert!(names(&fx).contains(&"DeleteCache"), "{prior:?}");
            assert!(m.data.items.is_empty(), "{prior:?}");
            assert!(m.view.revealed.is_none(), "{prior:?}");
            assert!(m.revealed_value(1).is_none(), "{prior:?}");
        }
    }

    #[test]
    fn signing_in_again_as_the_same_account_keeps_its_data() {
        let mut m = signed_in_as_one();
        m.update(Msg::RefreshFailed(PassError::Locked), 0);
        let fx = m.update(Msg::SessionProbed(Ok(account("one"))), 0);
        assert!(!names(&fx).contains(&"DeleteCache"), "nothing was switched");
        assert!(!m.data.items.is_empty());
        assert_eq!(m.revealed_value(1).map(|v| v.expose_secret()), Some("4111"));
    }

    #[test]
    fn signing_out_stops_a_reveal_fetch_in_flight() {
        let mut m = detail(vec![card("a")]);
        let fx = m.update(Msg::ToggleReveal, 0);
        let abandoned = reveal_id(&fx);
        let [Effect::FetchReveal { cancel, .. }] = &fx[..] else {
            panic!("the pane fetches the first secret");
        };
        let cancel = cancel.clone();
        m.update(Msg::RefreshFailed(PassError::SignedOut), 0);
        assert!(cancel.is_cancelled(), "the old account's fetch is stopped");
        reveal_fetched_for(&mut m, "a", abandoned, 1, "4111");
        assert!(m.view.revealed.is_none(), "a late result is dropped");
    }

    #[test]
    fn a_failed_reveal_lets_the_next_press_reach_the_following_field() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::ToggleReveal, 0);
        reveal_failed(&mut m, "a", 1);
        assert_eq!(
            m.view.notice.as_ref().map(|n| n.text.as_str()),
            Some(PassError::FieldMissing.to_string().as_str())
        );
        assert!(m.view.revealed.is_none());
        assert!(
            m.revealed_value(1).is_none(),
            "the failed field stays masked"
        );
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(
            revealing(&fx),
            Some((2, "cvv")),
            "the cycle moves past the failure instead of restarting"
        );
    }

    #[test]
    fn a_field_that_always_fails_does_not_trap_the_reveal_cycle() {
        let mut m = detail(vec![card("a")]);
        m.update(Msg::ToggleReveal, 0);
        reveal_fetched(&mut m, "a", 1, "4111");
        m.update(Msg::ToggleReveal, 0);
        reveal_failed(&mut m, "a", 2);
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(
            revealing(&fx),
            Some((3, "pin")),
            "the last secret is still reachable"
        );
        reveal_fetched(&mut m, "a", 3, "1234");
        assert_eq!(m.revealed_value(3).map(|v| v.expose_secret()), Some("1234"));
        let fx = m.update(Msg::ToggleReveal, 0);
        assert!(fx.is_empty(), "the press after the last secret re-masks");
    }

    #[test]
    fn a_reveal_that_fails_on_the_last_field_restarts_the_cycle() {
        let mut m = detail(vec![card("a")]);
        for index in [1, 2, 3] {
            m.update(Msg::ToggleReveal, 0);
            reveal_failed(&mut m, "a", index);
        }
        let fx = m.update(Msg::ToggleReveal, 0);
        assert!(fx.is_empty(), "past the last secret nothing is fetched");
        let fx = m.update(Msg::ToggleReveal, 0);
        assert_eq!(revealing(&fx), Some((1, "number")));
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
    fn an_errored_session_does_not_keep_refreshing() {
        // A probe that failed for an unclassified reason leaves the session in Error. Opening
        // the popup must not quietly fan out another pass-cli run behind that panel: on a
        // profile where pass-cli has never run, that is what turned one failure into a stream
        // of them. Recovery is the explicit Try again the status panel offers.
        let mut m = Model::default();
        m.update(
            Msg::SessionProbed(Err(PassError::Cli {
                message: "Error creating client features".into(),
            })),
            0,
        );
        assert!(matches!(m.session, SessionState::Error(_)));

        let fx = m.update(Msg::Show, 0);
        assert!(
            !names(&fx).contains(&"Refresh"),
            "opening the popup must not refresh while the session is in error"
        );
        assert!(
            m.update(Msg::RefreshRequested, 0).is_empty(),
            "nor must an explicit refresh request"
        );

        let fx = m.update(Msg::Startup, 0);
        assert!(
            names(&fx).contains(&"ProbeSession"),
            "Try again re-probes, which is how the user gets out of the error state"
        );
    }

    #[test]
    fn probe_errors_map_to_states() {
        for (err, state) in [
            (PassError::SignedOut, SessionState::SignedOut),
            (PassError::Locked, SessionState::Locked),
            (PassError::CliMissing, SessionState::CliMissing),
            (
                PassError::LocalData,
                SessionState::Error(PassError::LocalData.to_string()),
            ),
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

    /// A refresh that fails on pass-cli's own store must not pass for a Proton outage: the
    /// saved items can never refresh until the user resets it, so the panel has to say so.
    #[test]
    fn local_database_failure_shows_the_recovery_panel() {
        let mut m = loaded(vec![login("a", "A")]);
        m.session = SessionState::SignedIn(account("a"));
        m.update(Msg::RefreshFailed(PassError::LocalData), 0);
        let SessionState::Error(message) = &m.session else {
            panic!("expected an error panel, got {:?}", m.session);
        };
        assert!(message.contains("logout --force"), "{message}");
        assert_ne!(m.data.last_error, Some(RefreshError::Unreachable));
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
    fn session_probe_network_failure_records_the_cause() {
        // The probe path marks data stale too, so it must record why (FR-020).
        let mut m = loaded(vec![login("a", "A")]);
        m.session = SessionState::SignedIn(account("a"));
        m.update(Msg::SessionProbed(Err(PassError::Network)), 0);
        assert_eq!(m.data.last_error, Some(RefreshError::Unreachable));
        assert_eq!(
            m.stale_notice(),
            Some("Can\u{2019}t reach Proton Pass \u{2014} showing saved items. Press F5 to retry.")
        );
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
    fn the_editor_stays_usable_while_the_session_is_locked() {
        // The session panel stands in for the result list, never for the editor: a locked
        // session is exactly when a user reaches for the shortcuts (FR-027).
        use crate::config::Modifier;
        let mut m = loaded(vec![login("a", "A")]);
        m.update(Msg::RefreshFailed(PassError::Locked), 0);
        assert_eq!(m.session, SessionState::Locked);
        m.update(Msg::OpenPreferences, 0);
        assert_eq!(prefs_mode(&m), Some(None));
        assert_eq!(
            m.update(Msg::AdjustClipboardClear(10), 0).len(),
            1,
            "the timeout is still editable"
        );
        m.update(Msg::StartRebind(Action::CopyTotp), 0);
        let chord = KeyChord::new(&[Modifier::Ctrl], "t");
        assert!(saved(&m.update(Msg::ChordCaptured(chord.clone()), 0)).is_some());
        assert_eq!(m.prefs.chord(Action::CopyTotp), chord);
        m.update(Msg::Back, 0);
        assert_eq!(m.view.mode, Mode::List, "and the way back is the same");
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
