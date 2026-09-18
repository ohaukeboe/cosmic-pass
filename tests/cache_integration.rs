//! Integration tests for the encrypted metadata cache.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cosmic_pass::cache::keystore::{KeyStore, MemoryKeyStore};
use cosmic_pass::cache::store::CacheStore;
use cosmic_pass::core::usage::UsageRecord;
use cosmic_pass::model::{AccountId, CACHE_FORMAT_VERSION, CacheFile, ItemKey, ShareId};
use cosmic_pass::pass::parse::parse_items;

fn fixture_items() -> Vec<cosmic_pass::model::ItemSummary> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pass-cli/synthetic");
    let mut items = parse_items(
        &std::fs::read(dir.join("item-list-share-a.json")).unwrap(),
        &ShareId("share-a".into()),
        "Personal",
    )
    .unwrap();
    items.extend(
        parse_items(
            &std::fs::read(dir.join("item-list--share-b.json")).unwrap(),
            &ShareId("-share-b".into()),
            "Work",
        )
        .unwrap(),
    );
    items
}

fn cache_file(account: &str) -> CacheFile {
    CacheFile {
        format_version: CACHE_FORMAT_VERSION,
        account: AccountId(account.into()),
        fetched_at: 1_700_000_000,
        vaults: vec![],
        items: fixture_items(),
        usage: vec![UsageRecord {
            key: ItemKey::new("share-a", "login-github"),
            last_used: 5,
            count: 2,
        }],
    }
}

struct Setup {
    dir: tempfile::TempDir,
    keys: Arc<MemoryKeyStore>,
    store: CacheStore,
}

fn setup(keys: MemoryKeyStore) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(keys);
    let store = CacheStore::new(dir.path().join("cosmic-pass"), keys.clone());
    Setup { dir, keys, store }
}

impl Setup {
    fn file(&self) -> PathBuf {
        self.dir.path().join("cosmic-pass/cache.bin")
    }
}

#[tokio::test]
async fn save_then_load_round_trips() {
    let s = setup(MemoryKeyStore::default());
    let file = cache_file("acc");
    s.store.save(&file).await.unwrap();
    assert_eq!(s.store.load().await, Some(file));
}

#[tokio::test]
async fn permissions_are_private() {
    let s = setup(MemoryKeyStore::default());
    s.store.save(&cache_file("acc")).await.unwrap();
    let file_mode = std::fs::metadata(s.file()).unwrap().permissions().mode() & 0o777;
    let dir_mode = std::fs::metadata(s.file().parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(file_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
}

#[tokio::test]
async fn stray_temp_file_does_not_break_the_cache() {
    let s = setup(MemoryKeyStore::default());
    let file = cache_file("acc");
    s.store.save(&file).await.unwrap();
    std::fs::write(s.file().with_file_name(".tmpXYZ"), b"partial").unwrap();
    assert_eq!(s.store.load().await, Some(file));
}

#[tokio::test]
async fn no_keyring_means_no_disk_io() {
    let s = setup(MemoryKeyStore::unavailable());
    s.store.save(&cache_file("acc")).await.unwrap();
    assert!(!s.file().exists());
    assert!(s.store.load().await.is_none());
}

#[tokio::test]
async fn corrupt_file_is_deleted() {
    let s = setup(MemoryKeyStore::default());
    s.store.save(&cache_file("acc")).await.unwrap();
    let mut bytes = std::fs::read(s.file()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(s.file(), bytes).unwrap();
    assert!(s.store.load().await.is_none());
    assert!(!s.file().exists());
}

#[tokio::test]
async fn missing_key_deletes_unreadable_file() {
    let s = setup(MemoryKeyStore::default());
    s.store.save(&cache_file("acc")).await.unwrap();
    s.keys.delete().await.unwrap();
    assert!(s.store.load().await.is_none());
    assert!(!s.file().exists());
    assert!(!s.keys.has_key(), "load never creates a key");
}

#[tokio::test]
async fn delete_all_removes_file_and_key() {
    let s = setup(MemoryKeyStore::default());
    s.store.save(&cache_file("acc")).await.unwrap();
    s.store.delete_all().await;
    assert!(!s.file().exists());
    assert!(!s.keys.has_key());
}

#[tokio::test]
async fn plaintext_contains_no_secrets() {
    let s = setup(MemoryKeyStore::default());
    s.store.save(&cache_file("acc")).await.unwrap();
    let raw = std::fs::read(s.file()).unwrap();
    let key = s.keys.key(false).await.unwrap();
    let plain = cosmic_pass::cache::crypto::open(&key, &raw).unwrap();
    let text = String::from_utf8_lossy(&plain);
    assert!(text.contains("octocat"), "summary data is present");
    for marker in ["SECRET-FIXTURE-", "otpauth", "SECRETFIXTURE"] {
        assert!(!text.contains(marker), "{marker} leaked");
    }
    assert!(
        !String::from_utf8_lossy(&raw).contains("octocat"),
        "file is encrypted"
    );
}

#[tokio::test]
async fn debounced_saves_write_once() {
    let s = setup(MemoryKeyStore::default());
    let store = s.store.with_debounce(Duration::from_millis(200));
    let first = cache_file("acc");
    let mut second = cache_file("acc");
    second.fetched_at += 1;
    store.save_debounced(first);
    store.save_debounced(second.clone());
    assert!(!s.file().exists(), "nothing written before the delay");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(store.writes(), 1);
    assert_eq!(store.load().await, Some(second));
}

#[tokio::test]
async fn env_override_selects_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = CacheStore::default_dir_from(Some(dir.path().as_os_str().to_owned()));
    assert_eq!(path, dir.path());
    let default = CacheStore::default_dir_from(None);
    assert!(default.ends_with("cosmic-pass"));
}

#[tokio::test]
async fn locked_keyring_keeps_the_cache_file() {
    let s = setup(MemoryKeyStore::default());
    let file = cache_file("acc");
    s.store.save(&file).await.unwrap();
    s.keys.set_unavailable(true);
    assert!(s.store.load().await.is_none());
    assert!(
        s.file().exists(),
        "a locked keyring must not destroy a readable cache"
    );
    s.keys.set_unavailable(false);
    assert_eq!(s.store.load().await, Some(file), "readable once unlocked");
}
