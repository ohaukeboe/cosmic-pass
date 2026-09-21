//! High-level Proton Pass operations built on `pass-cli`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::error::PassError;
use super::runner::{BoxFuture, CommandRunner};
use crate::model::{AccountId, CliVersion, ItemKey, ItemSummary, Vault};

type Result<T> = std::result::Result<T, PassError>;

/// All active items across all vaults.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub vaults: Vec<Vault>,
    pub items: Vec<ItemSummary>,
}

pub trait PassBackend: Send + Sync {
    /// The signed-in account. Fails with [`PassError::SignedOut`] when signed out.
    fn account(&self) -> BoxFuture<'_, Result<AccountId>>;
    /// The installed `pass-cli`'s version. Needs no session.
    fn version(&self) -> BoxFuture<'_, Result<CliVersion>>;
    /// Lists every vault and its active items. Fails if any vault listing fails.
    fn list_all(&self) -> BoxFuture<'_, Result<Listing>>;
    fn get_field(
        &self,
        key: ItemKey,
        field: String,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<SecretString>>;
    /// Current one-time codes, keyed by TOTP field name.
    fn totp(
        &self,
        key: ItemKey,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<BTreeMap<String, SecretString>>>;
    /// Runs the web sign-in flow, forwarding each output line to `lines`.
    fn login(&self, lines: mpsc::Sender<String>) -> BoxFuture<'_, Result<()>>;
}

pub struct PassCli<R> {
    runner: Arc<R>,
}

impl<R: CommandRunner + 'static> PassCli<R> {
    pub fn new(runner: Arc<R>) -> Self {
        Self { runner }
    }

    async fn run(
        &self,
        args: Vec<String>,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<super::runner::Output> {
        self.runner.run(args, timeout, cancel).await
    }
}

const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const INFO_TIMEOUT: Duration = Duration::from_secs(5);
const LIST_TIMEOUT: Duration = Duration::from_secs(20);
const FIELD_TIMEOUT: Duration = Duration::from_secs(10);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

fn args<const N: usize>(parts: [&str; N]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_owned()).collect()
}

fn id_args(key: &ItemKey) -> [String; 2] {
    [
        format!("--share-id={}", key.share.0),
        format!("--item-id={}", key.item.0),
    ]
}

/// Every `pass-cli` command line this app sends, in one place.
///
/// `pass-cli` publishes no stability policy, so these argv are the app's most fragile
/// assumption, and they have to be assertable from outside. `tests/pass_cli_contract.rs` runs
/// each one against the real binary to prove it still parses; sourcing them from here rather
/// than retyping them is what makes that test fail when this file changes.
///
/// IDs are always `--flag=VALUE`: a share id can begin with `-`, and the space-separated form
/// then fails with `unexpected argument`.
pub mod argv {
    use crate::model::ItemKey;

    pub fn version() -> Vec<String> {
        super::args(["--version"])
    }

    pub fn info() -> Vec<String> {
        super::args(["info", "--output", "json"])
    }

    pub fn vault_list() -> Vec<String> {
        super::args(["vault", "list", "--output", "json"])
    }

    pub fn item_list(share: &str) -> Vec<String> {
        vec![
            "item".into(),
            "list".into(),
            format!("--share-id={share}"),
            "--output".into(),
            "json".into(),
            "--show-secrets".into(),
        ]
    }

    pub fn item_view(key: &ItemKey, field: &str) -> Vec<String> {
        let [share, item] = super::id_args(key);
        vec![
            "item".into(),
            "view".into(),
            share,
            item,
            format!("--field={field}"),
        ]
    }

    pub fn item_totp(key: &ItemKey) -> Vec<String> {
        let [share, item] = super::id_args(key);
        vec![
            "item".into(),
            "totp".into(),
            share,
            item,
            "--output".into(),
            "json".into(),
        ]
    }

    pub fn login() -> Vec<String> {
        super::args(["login"])
    }
}

impl<R: CommandRunner + 'static> PassBackend for PassCli<R> {
    fn account(&self) -> BoxFuture<'_, Result<AccountId>> {
        Box::pin(async move {
            let out = self
                .run(argv::info(), INFO_TIMEOUT, CancellationToken::new())
                .await?;
            super::parse::parse_account(&out.stdout)
        })
    }

    fn version(&self) -> BoxFuture<'_, Result<CliVersion>> {
        Box::pin(async move {
            let out = self
                .run(argv::version(), VERSION_TIMEOUT, CancellationToken::new())
                .await?;
            super::parse::parse_version(&out.stdout)
        })
    }

    fn list_all(&self) -> BoxFuture<'_, Result<Listing>> {
        Box::pin(async move {
            let out = self
                .run(argv::vault_list(), LIST_TIMEOUT, CancellationToken::new())
                .await?;
            let vaults = super::parse::parse_vaults(&out.stdout)?;
            // Dropping the remaining futures on the first error cancels their processes.
            let per_vault = futures::future::try_join_all(vaults.iter().map(|vault| async move {
                let out = self
                    .run(
                        argv::item_list(&vault.share_id.0),
                        LIST_TIMEOUT,
                        CancellationToken::new(),
                    )
                    .await?;
                super::parse::parse_items(&out.stdout, &vault.share_id, &vault.name)
            }))
            .await?;
            Ok(Listing {
                items: per_vault.into_iter().flatten().collect(),
                vaults,
            })
        })
    }

    fn get_field(
        &self,
        key: ItemKey,
        field: String,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<SecretString>> {
        Box::pin(async move {
            let out = self
                .run(argv::item_view(&key, &field), FIELD_TIMEOUT, cancel)
                .await?;
            super::parse::parse_field(&out.stdout)
        })
    }

    fn totp(
        &self,
        key: ItemKey,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<BTreeMap<String, SecretString>>> {
        Box::pin(async move {
            let out = self
                .run(argv::item_totp(&key), FIELD_TIMEOUT, cancel)
                .await?;
            super::parse::parse_totp(&out.stdout)
        })
    }

    fn login(&self, lines: mpsc::Sender<String>) -> BoxFuture<'_, Result<()>> {
        self.runner
            .run_streaming(argv::login(), LOGIN_TIMEOUT, lines)
    }
}
