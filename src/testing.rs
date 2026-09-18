//! Test doubles for the IO boundaries and a harness that runs the reducer with them.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::clipboard::{Clipboard, ClipboardError};
use crate::config::Preferences;
use crate::core::effects::Effect;
use crate::core::state::{Model, Msg};
use crate::model::{AccountId, ItemKey, ItemKind, ItemSummary, ShareId, Vault};
use crate::pass::backend::{Listing, PassBackend};
use crate::pass::error::PassError;
use crate::pass::runner::BoxFuture;
use crate::runtime::{Deps, Step, execute};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A login-less summary in vault "Personal" with share `s`.
pub fn summary(id: &str, title: &str, kind: ItemKind) -> ItemSummary {
    ItemSummary {
        key: ItemKey::new("s", id),
        vault_name: "Personal".into(),
        kind,
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

pub fn listing(items: Vec<ItemSummary>) -> Listing {
    Listing {
        vaults: vec![Vault {
            share_id: ShareId("s".into()),
            name: "Personal".into(),
        }],
        items,
    }
}

struct BackendState {
    account: Result<AccountId, PassError>,
    listing: Result<Listing, PassError>,
    fields: HashMap<(ItemKey, String), Result<String, PassError>>,
    totp: HashMap<ItemKey, Result<BTreeMap<String, String>, PassError>>,
    login_lines: Vec<String>,
    login_result: Result<(), PassError>,
    delay: Duration,
    list_calls: usize,
    field_calls: usize,
    totp_calls: usize,
    login_calls: usize,
}

/// In-memory [`PassBackend`] with scriptable results and delays.
pub struct FakeBackend {
    state: Mutex<BackendState>,
}

impl FakeBackend {
    pub fn with_items(items: Vec<ItemSummary>) -> Self {
        Self {
            state: Mutex::new(BackendState {
                account: Ok(AccountId("account-1".into())),
                listing: Ok(listing(items)),
                fields: HashMap::new(),
                totp: HashMap::new(),
                login_lines: Vec::new(),
                login_result: Ok(()),
                delay: Duration::ZERO,
                list_calls: 0,
                field_calls: 0,
                totp_calls: 0,
                login_calls: 0,
            }),
        }
    }

    pub fn set_listing(&self, listing: Result<Listing, PassError>) {
        lock(&self.state).listing = listing;
    }

    pub fn set_account(&self, account: Result<AccountId, PassError>) {
        lock(&self.state).account = account;
    }

    pub fn set_field(&self, key: &ItemKey, field: &str, value: &str) {
        lock(&self.state)
            .fields
            .insert((key.clone(), field.into()), Ok(value.into()));
    }

    pub fn fail_field(&self, key: &ItemKey, field: &str, error: PassError) {
        lock(&self.state)
            .fields
            .insert((key.clone(), field.into()), Err(error));
    }

    pub fn set_totp(&self, key: &ItemKey, codes: &[(&str, &str)]) {
        let map = codes
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        lock(&self.state).totp.insert(key.clone(), Ok(map));
    }

    pub fn set_login(&self, lines: &[&str], result: Result<(), PassError>) {
        let mut s = lock(&self.state);
        s.login_lines = lines.iter().map(|l| (*l).to_owned()).collect();
        s.login_result = result;
    }

    pub fn set_delay(&self, delay: Duration) {
        lock(&self.state).delay = delay;
    }

    pub fn list_calls(&self) -> usize {
        lock(&self.state).list_calls
    }

    pub fn field_calls(&self) -> usize {
        lock(&self.state).field_calls
    }

    pub fn totp_calls(&self) -> usize {
        lock(&self.state).totp_calls
    }

    pub fn login_calls(&self) -> usize {
        lock(&self.state).login_calls
    }

    fn delay(&self) -> Duration {
        lock(&self.state).delay
    }
}

async fn wait(delay: Duration, cancel: &CancellationToken) -> Result<(), PassError> {
    tokio::select! {
        () = tokio::time::sleep(delay) => Ok(()),
        () = cancel.cancelled() => Err(PassError::Cancelled),
    }
}

impl PassBackend for FakeBackend {
    fn account(&self) -> BoxFuture<'_, Result<AccountId, PassError>> {
        Box::pin(async move { lock(&self.state).account.clone() })
    }

    fn list_all(&self) -> BoxFuture<'_, Result<Listing, PassError>> {
        Box::pin(async move {
            let delay = self.delay();
            tokio::time::sleep(delay).await;
            let mut s = lock(&self.state);
            s.list_calls += 1;
            s.listing.clone()
        })
    }

    fn get_field(
        &self,
        key: ItemKey,
        field: String,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<SecretString, PassError>> {
        Box::pin(async move {
            lock(&self.state).field_calls += 1;
            wait(self.delay(), &cancel).await?;
            lock(&self.state)
                .fields
                .get(&(key, field))
                .cloned()
                .unwrap_or(Err(PassError::FieldMissing))
                .map(SecretString::from)
        })
    }

    fn totp(
        &self,
        key: ItemKey,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<BTreeMap<String, SecretString>, PassError>> {
        Box::pin(async move {
            lock(&self.state).totp_calls += 1;
            wait(self.delay(), &cancel).await?;
            let codes = lock(&self.state)
                .totp
                .get(&key)
                .cloned()
                .unwrap_or_else(|| Ok(BTreeMap::new()))?;
            Ok(codes
                .into_iter()
                .map(|(k, v)| (k, SecretString::from(v)))
                .collect())
        })
    }

    fn login(&self, lines: mpsc::Sender<String>) -> BoxFuture<'_, Result<(), PassError>> {
        Box::pin(async move {
            let (out, result) = {
                let mut s = lock(&self.state);
                s.login_calls += 1;
                (s.login_lines.clone(), s.login_result.clone())
            };
            for line in out {
                let _ = lines.send(line).await;
            }
            result
        })
    }
}

/// Records copies instead of touching the real clipboard.
#[derive(Default)]
pub struct FakeClipboard {
    copies: Mutex<Vec<(String, bool, Duration)>>,
    fail: Mutex<bool>,
}

impl FakeClipboard {
    pub fn copies(&self) -> Vec<(String, bool, Duration)> {
        lock(&self.copies).clone()
    }

    pub fn fail(&self) {
        *lock(&self.fail) = true;
    }
}

impl Clipboard for FakeClipboard {
    fn copy(
        &self,
        value: SecretString,
        secret: bool,
        clear_after: Duration,
    ) -> BoxFuture<'_, Result<(), ClipboardError>> {
        Box::pin(async move {
            if *lock(&self.fail) {
                return Err(ClipboardError::Unavailable);
            }
            lock(&self.copies).push((value.expose_secret().to_owned(), secret, clear_after));
            Ok(())
        })
    }
}

/// Runs [`Model::update`] and executes its effects against the fakes.
pub struct Harness {
    pub model: Model,
    pub backend: Arc<FakeBackend>,
    pub clipboard: Arc<FakeClipboard>,
    deps: Deps,
    tasks: JoinSet<Vec<Msg>>,
    visible: bool,
    persisted: usize,
    cache_deletions: usize,
    preference_saves: usize,
    messages: Vec<String>,
    now: i64,
    unhandled: Vec<String>,
}

impl Harness {
    pub fn new(backend: FakeBackend) -> Self {
        Self::with_prefs(backend, Preferences::default())
    }

    pub fn with_prefs(backend: FakeBackend, prefs: Preferences) -> Self {
        let backend = Arc::new(backend);
        let clipboard = Arc::new(FakeClipboard::default());
        let deps = Deps {
            backend: backend.clone(),
            clipboard: clipboard.clone(),
            cache: None,
            save_preferences: false,
        };
        Self {
            model: Model::new(prefs),
            backend,
            clipboard,
            deps,
            tasks: JoinSet::new(),
            visible: false,
            persisted: 0,
            cache_deletions: 0,
            preference_saves: 0,
            messages: Vec::new(),
            now: 1_000_000,
            unhandled: Vec::new(),
        }
    }

    pub fn now(&self) -> i64 {
        self.now
    }

    pub fn advance(&mut self, secs: i64) {
        self.now += secs;
    }

    pub fn window_visible(&self) -> bool {
        self.visible
    }

    /// Number of `Persist` effects seen.
    pub fn persisted(&self) -> usize {
        self.persisted
    }

    /// Number of `DeleteCache` effects seen.
    pub fn cache_deletions(&self) -> usize {
        self.cache_deletions
    }

    /// Number of `SavePreferences` effects seen.
    pub fn preference_saves(&self) -> usize {
        self.preference_saves
    }

    /// `Debug` text of every message processed, in order. Secrets print as redacted.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Names of effects the harness could not execute, in order.
    pub fn unhandled(&self) -> &[String] {
        &self.unhandled
    }

    pub fn result_ids(&self) -> Vec<String> {
        self.model
            .view
            .results
            .iter()
            .map(|r| self.model.data.items[r.item].key.item.0.clone())
            .collect()
    }

    /// Updates the model and starts effects without waiting for them.
    pub fn send_no_wait(&mut self, msg: Msg) {
        self.messages.push(format!("{msg:?}"));
        let effects = self.model.update(msg, self.now);
        for effect in effects {
            match effect {
                Effect::Persist => self.persisted += 1,
                Effect::DeleteCache => self.cache_deletions += 1,
                Effect::LoadCache => {}
                Effect::SavePreferences(_) => self.preference_saves += 1,
                // Notices would make every test wait; expiry is tested in the reducer.
                Effect::ExpireNotice { .. } => {}
                effect => match execute(effect, &self.deps, &self.model) {
                    Step::ShowWindow => self.visible = true,
                    Step::HideWindow => self.visible = false,
                    Step::Future(f) => {
                        self.tasks
                            .spawn(async move { f.await.into_iter().collect() });
                    }
                    Step::Stream(stream) => {
                        use futures::StreamExt;
                        self.tasks.spawn(stream.collect::<Vec<_>>());
                    }
                    Step::Unhandled(e) => self.unhandled.push(format!("{e:?}")),
                },
            }
        }
    }

    /// Updates the model and runs all resulting effects to completion.
    pub async fn send(&mut self, msg: Msg) {
        self.send_no_wait(msg);
        self.settle().await;
    }

    /// Waits for running effects and feeds their messages back until none remain.
    pub async fn settle(&mut self) {
        while let Some(result) = self.tasks.join_next().await {
            for msg in result.expect("effect task panicked") {
                self.send_no_wait(msg);
            }
        }
    }
}
