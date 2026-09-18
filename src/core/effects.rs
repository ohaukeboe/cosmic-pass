//! Side effects requested by the reducer and executed by the app layer.

use std::time::Duration;

use secrecy::SecretString;
use tokio_util::sync::CancellationToken;

use crate::model::ItemKey;

#[derive(Debug)]
pub enum Effect {
    ShowWindow,
    HideWindow,
    /// List all vaults and items.
    Refresh,
    /// Check which account is signed in.
    ProbeSession,
    /// Fetch one field with `pass-cli` and copy it.
    FetchAndCopy {
        key: ItemKey,
        field: String,
        secret: bool,
        cancel: CancellationToken,
    },
    /// Fetch one-time codes and copy the code of `field`.
    FetchTotpAndCopy {
        key: ItemKey,
        field: String,
        cancel: CancellationToken,
    },
    /// Put a value on the clipboard. Secret values are cleared after the timeout.
    Copy {
        value: SecretString,
        secret: bool,
    },
    /// Save the metadata cache.
    Persist,
    LoadCache,
    DeleteCache,
    StartLogin,
    /// Fetch a secret field for the detail pane.
    FetchReveal {
        key: ItemKey,
        field: String,
        cancel: CancellationToken,
    },
    /// Fetch one-time codes for the detail pane.
    FetchTotp {
        key: ItemKey,
        cancel: CancellationToken,
    },
    /// Write preferences to cosmic-config.
    SavePreferences(crate::config::Preferences),
    /// Send `Msg::NoticeExpired(id)` after `after`.
    ExpireNotice {
        id: u64,
        after: Duration,
    },
}
