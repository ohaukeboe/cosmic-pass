//! Cache encryption key stored in the Secret Service.

use std::collections::HashMap;
use std::sync::Mutex;

use super::CacheError;
use super::crypto::{CacheKey, KEY_LEN, generate_key};
use crate::pass::runner::BoxFuture;

pub const ATTR_APPLICATION: &str = "io.github.ohaukeboe.CosmicPass";
pub const ATTR_PURPOSE: &str = "cache-key";
const LABEL: &str = "COSMIC Pass cache key";

pub trait KeyStore: Send + Sync {
    /// The cache key; created when missing and `create` is set. `None` when the keyring is
    /// unavailable or locked (FR-024a).
    fn key(&self, create: bool) -> BoxFuture<'_, Option<CacheKey>>;
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
    async fn key_inner(&self, create: bool) -> Result<Option<CacheKey>, CacheError> {
        let err = |e: oo7::Error| CacheError::Keyring(e.to_string());
        let keyring = oo7::Keyring::new().await.map_err(err)?;
        if keyring.is_locked().await.map_err(err)? {
            return Ok(None);
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
                return Ok(None);
            }
            let secret = item.secret().await.map_err(err)?;
            let bytes: [u8; KEY_LEN] = secret
                .as_bytes()
                .try_into()
                .map_err(|_| CacheError::Keyring("stored key has the wrong length".into()))?;
            return Ok(Some(zeroize::Zeroizing::new(bytes)));
        }
        if !create {
            return Ok(None);
        }
        let key = generate_key();
        keyring
            .create_item(LABEL, &attrs, key.to_vec(), true)
            .await
            .map_err(err)?;
        Ok(Some(key))
    }
}

impl KeyStore for Oo7KeyStore {
    fn key(&self, create: bool) -> BoxFuture<'_, Option<CacheKey>> {
        Box::pin(async move {
            if disabled() {
                return None;
            }
            match self.key_inner(create).await {
                Ok(key) => key,
                Err(e) => {
                    tracing::warn!("cache key unavailable: {e}");
                    None
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

    fn lock_key(&self) -> std::sync::MutexGuard<'_, Option<[u8; KEY_LEN]>> {
        self.key
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl KeyStore for MemoryKeyStore {
    fn key(&self, create: bool) -> BoxFuture<'_, Option<CacheKey>> {
        Box::pin(async move {
            if *self
                .unavailable
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
            {
                return None;
            }
            let mut slot = self.lock_key();
            if slot.is_none() && create {
                *slot = Some(*generate_key());
            }
            slot.map(zeroize::Zeroizing::new)
        })
    }

    fn delete(&self) -> BoxFuture<'_, Result<(), CacheError>> {
        Box::pin(async move {
            *self.lock_key() = None;
            Ok(())
        })
    }
}
