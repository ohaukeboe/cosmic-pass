//! Loading, saving, and deleting the cache file.

use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::CacheError;
use super::crypto;
use super::keystore::KeyStore;
use crate::model::{CACHE_FORMAT_VERSION, CacheFile};

const FILE_NAME: &str = "cache.bin";
const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Debounce {
    pending: Option<CacheFile>,
    scheduled: bool,
}

struct Inner {
    dir: PathBuf,
    keys: Arc<dyn KeyStore>,
    debounce: Duration,
    queue: Mutex<Debounce>,
    writes: AtomicUsize,
}

/// The encrypted cache at `<dir>/cache.bin`. Cheap to clone.
#[derive(Clone)]
pub struct CacheStore {
    inner: Arc<Inner>,
}

fn io(e: impl std::fmt::Display) -> CacheError {
    CacheError::Io(e.to_string())
}

impl CacheStore {
    pub fn new(dir: impl Into<PathBuf>, keys: Arc<dyn KeyStore>) -> Self {
        Self::build(dir.into(), keys, DEFAULT_DEBOUNCE)
    }

    fn build(dir: PathBuf, keys: Arc<dyn KeyStore>, debounce: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                dir,
                keys,
                debounce,
                queue: Mutex::new(Debounce::default()),
                writes: AtomicUsize::new(0),
            }),
        }
    }

    /// Uses `$COSMIC_PASS_CACHE_DIR`, else `$XDG_CACHE_HOME/cosmic-pass`.
    pub fn from_env(keys: Arc<dyn KeyStore>) -> Self {
        Self::new(
            Self::default_dir_from(std::env::var_os("COSMIC_PASS_CACHE_DIR")),
            keys,
        )
    }

    pub fn default_dir_from(env_override: Option<OsString>) -> PathBuf {
        env_override.map_or_else(
            || {
                dirs::cache_dir()
                    .unwrap_or_else(|| PathBuf::from(".cache"))
                    .join("cosmic-pass")
            },
            PathBuf::from,
        )
    }

    /// A copy with a different debounce delay (tests).
    pub fn with_debounce(&self, delay: Duration) -> Self {
        Self::build(self.inner.dir.clone(), self.inner.keys.clone(), delay)
    }

    fn path(&self) -> PathBuf {
        self.inner.dir.join(FILE_NAME)
    }

    /// Number of files written so far.
    pub fn writes(&self) -> usize {
        self.inner.writes.load(Ordering::Relaxed)
    }

    /// Reads the cache. Unreadable or incompatible files are deleted. Never creates a key.
    pub async fn load(&self) -> Option<CacheFile> {
        let path = self.path();
        let raw = match tokio::fs::read(&path).await {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                tracing::warn!("reading cache: {e}");
                return None;
            }
        };
        let Some(key) = self.inner.keys.key(false).await else {
            // Without the key the file can never be read again.
            remove(&path).await;
            return None;
        };
        let decoded = crypto::open(&key, &raw).and_then(|plain| {
            postcard::from_bytes::<CacheFile>(&plain).map_err(|_| CacheError::Corrupt)
        });
        match decoded {
            Ok(file) if file.format_version == CACHE_FORMAT_VERSION => Some(file),
            Ok(_) | Err(_) => {
                tracing::info!("discarding unreadable cache file");
                remove(&path).await;
                None
            }
        }
    }

    /// Encrypts and writes the cache atomically. Does nothing without a keyring (FR-024a).
    pub async fn save(&self, file: &CacheFile) -> Result<(), CacheError> {
        let Some(key) = self.inner.keys.key(true).await else {
            return Ok(());
        };
        let plain = zeroize::Zeroizing::new(postcard::to_allocvec(file).map_err(io)?);
        let sealed = crypto::seal(&key, &plain)?;
        let dir = self.inner.dir.clone();
        tokio::task::spawn_blocking(move || write_atomic(&dir, &sealed))
            .await
            .map_err(io)??;
        self.inner.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Saves `file` after the debounce delay; later calls within the delay replace it.
    pub fn save_debounced(&self, file: CacheFile) {
        let mut queue = self
            .inner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.pending = Some(file);
        if queue.scheduled {
            return;
        }
        queue.scheduled = true;
        let store = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(store.inner.debounce).await;
            let file = {
                let mut queue = store
                    .inner
                    .queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                queue.scheduled = false;
                queue.pending.take()
            };
            if let Some(file) = file
                && let Err(e) = store.save(&file).await
            {
                tracing::warn!("saving cache: {e}");
            }
        });
    }

    /// Removes the cache file and its key (sign-out or account switch, FR-024b).
    pub async fn delete_all(&self) {
        {
            let mut queue = self
                .inner
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue.pending = None;
        }
        remove(&self.path()).await;
        if let Err(e) = self.inner.keys.delete().await {
            tracing::warn!("deleting cache key: {e}");
        }
    }
}

async fn remove(path: &Path) {
    if let Err(e) = tokio::fs::remove_file(path).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("removing cache file: {e}");
    }
}

fn write_atomic(dir: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(io)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".tmp")
        .permissions(std::fs::Permissions::from_mode(0o600))
        .tempfile_in(dir)
        .map_err(io)?;
    tmp.write_all(bytes).map_err(io)?;
    tmp.as_file().sync_all().map_err(io)?;
    tmp.persist(dir.join(FILE_NAME)).map_err(io)?;
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io)?;
    Ok(())
}
