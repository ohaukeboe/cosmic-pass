//! High-level Proton Pass operations built on `pass-cli`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::error::PassError;
use super::runner::{BoxFuture, CommandRunner};
use crate::model::{AccountId, ItemKey, ItemSummary, Vault};

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

impl<R: CommandRunner + 'static> PassBackend for PassCli<R> {
    fn account(&self) -> BoxFuture<'_, Result<AccountId>> {
        Box::pin(async move {
            let out = self
                .run(
                    args(["info", "--output", "json"]),
                    INFO_TIMEOUT,
                    CancellationToken::new(),
                )
                .await?;
            super::parse::parse_account(&out.stdout)
        })
    }

    fn list_all(&self) -> BoxFuture<'_, Result<Listing>> {
        Box::pin(async move {
            let out = self
                .run(
                    args(["vault", "list", "--output", "json"]),
                    LIST_TIMEOUT,
                    CancellationToken::new(),
                )
                .await?;
            let vaults = super::parse::parse_vaults(&out.stdout)?;
            // Dropping the remaining futures on the first error cancels their processes.
            let per_vault = futures::future::try_join_all(vaults.iter().map(|vault| async move {
                let out = self
                    .run(
                        vec![
                            "item".into(),
                            "list".into(),
                            format!("--share-id={}", vault.share_id.0),
                            "--output".into(),
                            "json".into(),
                            "--show-secrets".into(),
                        ],
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
            let [share, item] = id_args(&key);
            let argv = vec![
                "item".into(),
                "view".into(),
                share,
                item,
                format!("--field={field}"),
            ];
            let out = self.run(argv, FIELD_TIMEOUT, cancel).await?;
            super::parse::parse_field(&out.stdout)
        })
    }

    fn totp(
        &self,
        key: ItemKey,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<BTreeMap<String, SecretString>>> {
        Box::pin(async move {
            let [share, item] = id_args(&key);
            let argv = vec![
                "item".into(),
                "totp".into(),
                share,
                item,
                "--output".into(),
                "json".into(),
            ];
            let out = self.run(argv, FIELD_TIMEOUT, cancel).await?;
            super::parse::parse_totp(&out.stdout)
        })
    }

    fn login(&self, lines: mpsc::Sender<String>) -> BoxFuture<'_, Result<()>> {
        self.runner
            .run_streaming(args(["login"]), LOGIN_TIMEOUT, lines)
    }
}
