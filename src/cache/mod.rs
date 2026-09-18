//! Encrypted on-disk cache of non-secret item metadata.

pub mod crypto;
pub mod keystore;
pub mod store;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CacheError {
    #[error("no cache key available")]
    NoKey,
    #[error("cache file is corrupt or was not written with this key")]
    Corrupt,
    #[error("cache file format is not supported")]
    Unsupported,
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("cache I/O error: {0}")]
    Io(String),
}
