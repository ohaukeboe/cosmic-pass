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
    /// Read the installed `pass-cli`'s version, to warn about untested ones.
    ProbeVersion,
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
    /// Fetch a secret field for the field list. `index` is the field's position in the
    /// item's fields and `generation` identifies this fetch; the result carries both back so
    /// the reducer can tell whose value it is.
    FetchReveal {
        key: ItemKey,
        field: String,
        index: usize,
        generation: u64,
        cancel: CancellationToken,
    },
    /// Fetch one-time codes for the revealed row of the field list.
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
