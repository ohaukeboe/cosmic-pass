//! Cache encryption key stored in the Secret Service.

use std::collections::HashMap;
use std::sync::Mutex;

use super::CacheError;
use super::crypto::{CacheKey, KEY_LEN, generate_key};
use crate::pass::runner::BoxFuture;

pub const ATTR_APPLICATION: &str = "io.github.ohaukeboe.CosmicPass";
pub const ATTR_PURPOSE: &str = "cache-key";
const LABEL: &str = "COSMIC Pass cache key";

/// Why no cache key is in hand. The two cases differ for an existing cache file: a key the
/// keyring does not hold is gone for good, while an unreachable keyring may still hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// The keyring answered and holds no key for us.
    Missing,
    /// The keyring is locked, disabled, or failed to answer (FR-024a).
    Unavailable,
}

/// The cache key, or why it could not be obtained.
pub type KeyOutcome = Result<CacheKey, KeyState>;

pub trait KeyStore: Send + Sync {
    /// The cache key; created when missing and `create` is set.
    fn key(&self, create: bool) -> BoxFuture<'_, KeyOutcome>;
    fn delete(&self) -> BoxFuture<'_, Result<(), CacheError>>;
}

/// Keyring access through `oo7` (Secret Service over D-Bus).
#[derive(Default)]
pub struct Oo7KeyStore;

fn attributes() -> HashMap<&'static str, &'static str> {
    HashMap::from([("application", ATTR_APPLICATION), ("purpose", ATTR_PURPOSE)])
}

fn disabled() -> bool {
    std::env::var_os("COSMIC_PASS_NO_KEYRING").is_some_and(|v| v == "1")
}

impl Oo7KeyStore {
    async fn key_inner(&self, create: bool) -> Result<KeyOutcome, CacheError> {
        let err = |e: oo7::Error| CacheError::Keyring(e.to_string());
        let keyring = oo7::Keyring::new().await.map_err(err)?;
        if keyring.is_locked().await.map_err(err)? {
            return Ok(Err(KeyState::Unavailable));
        }
        let attrs = attributes();
        if let Some(item) = keyring
            .search_items(&attrs)
            .await
            .map_err(err)?
            .into_iter()
            .next()
        {
            if item.is_locked().await.map_err(err)? {
                return Ok(Err(KeyState::Unavailable));
            }
            let secret = item.secret().await.map_err(err)?;
            let bytes: [u8; KEY_LEN] = secret
                .as_bytes()
                .try_into()
                .map_err(|_| CacheError::Keyring("stored key has the wrong length".into()))?;
            return Ok(Ok(zeroize::Zeroizing::new(bytes)));
        }
        if !create {
            return Ok(Err(KeyState::Missing));
        }
        let key = generate_key();
        keyring
            .create_item(LABEL, &attrs, key.to_vec(), true)
            .await
            .map_err(err)?;
        Ok(Ok(key))
    }
}

impl KeyStore for Oo7KeyStore {
    fn key(&self, create: bool) -> BoxFuture<'_, KeyOutcome> {
        Box::pin(async move {
            if disabled() {
                return Err(KeyState::Unavailable);
            }
            match self.key_inner(create).await {
                Ok(outcome) => outcome,
                Err(e) => {
                    tracing::warn!("cache key unavailable: {e}");
                    Err(KeyState::Unavailable)
                }
            }
        })
    }

    fn delete(&self) -> BoxFuture<'_, Result<(), CacheError>> {
        Box::pin(async move {
            if disabled() {
                return Ok(());
            }
            let err = |e: oo7::Error| CacheError::Keyring(e.to_string());
            let keyring = oo7::Keyring::new().await.map_err(err)?;
            keyring.delete(&attributes()).await.map_err(err)
        })
    }
}

/// In-memory key store for tests.
#[derive(Default)]
pub struct MemoryKeyStore {
    key: Mutex<Option<[u8; KEY_LEN]>>,
    unavailable: Mutex<bool>,
}

impl MemoryKeyStore {
    pub fn unavailable() -> Self {
        Self {
            unavailable: Mutex::new(true),
            ..Self::default()
        }
    }

    pub fn has_key(&self) -> bool {
        self.lock_key().is_some()
    }

    /// Simulates the keyring locking or unlocking after the key was stored.
    pub fn set_unavailable(&self, unavailable: bool) {
        *self
            .unavailable
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = unavailable;
    }

    fn lock_key(&self) -> std::sync::MutexGuard<'_, Option<[u8; KEY_LEN]>> {
        self.key
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl KeyStore for MemoryKeyStore {
    fn key(&self, create: bool) -> BoxFuture<'_, KeyOutcome> {
        Box::pin(async move {
            if *self
                .unavailable
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
            {
                return Err(KeyState::Unavailable);
            }
            let mut slot = self.lock_key();
            if slot.is_none() && create {
                *slot = Some(*generate_key());
            }
            slot.map(zeroize::Zeroizing::new).ok_or(KeyState::Missing)
        })
    }

    fn delete(&self) -> BoxFuture<'_, Result<(), CacheError>> {
        Box::pin(async move {
            *self.lock_key() = None;
            Ok(())
        })
    }
}
