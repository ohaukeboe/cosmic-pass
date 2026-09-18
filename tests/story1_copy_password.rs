//! Acceptance tests for User Story 1: find a login and copy its password.

use std::time::Duration;

use cosmic_pass::core::state::{Mode, Msg};
use cosmic_pass::model::{FieldRef, ItemKey, ItemKind, ItemSummary};
use cosmic_pass::pass::error::PassError;
use cosmic_pass::testing::{FakeBackend, Harness, summary};

fn login(id: &str, title: &str, user: &str, url: &str) -> ItemSummary {
    let mut item = summary(id, title, ItemKind::Login);
    item.username = Some(user.into());
    item.subtitle = Some(user.into());
    item.urls = vec![url.into()];
    item.fields = vec![
        FieldRef::plain("username", "Username", user.into()),
        FieldRef::secret("password", "Password"),
    ];
    item
}

fn backend() -> FakeBackend {
    let backend = FakeBackend::with_items(vec![
        login("gh", "GitHub", "octocat", "https://github.com"),
        login("mail", "Mail", "gituser", "https://mail.example"),
        login("bank", "Bank", "me", "https://bank.example"),
    ]);
    backend.set_field(&key("gh"), "password", "SECRET-FIXTURE-gh");
    backend.set_field(&key("bank"), "password", "SECRET-FIXTURE-bank");
    backend
}

fn key(id: &str) -> ItemKey {
    ItemKey::new("s", id)
}

async fn opened() -> Harness {
    let mut h = Harness::new(backend());
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Toggle).await;
    h
}

#[tokio::test]
async fn toggle_shows_window_with_empty_query() {
    let h = opened().await;
    assert!(h.window_visible());
    assert_eq!(h.model.view.query, "");
    assert_eq!(h.model.view.results.len(), 3);
}

#[tokio::test]
async fn typing_filters_best_title_match_first() {
    let mut h = opened().await;
    h.send(Msg::QueryChanged("git".into())).await;
    assert_eq!(h.result_ids(), ["gh", "mail"]);
}

#[tokio::test]
async fn enter_copies_password_and_hides() {
    let mut h = opened().await;
    h.send(Msg::QueryChanged("github".into())).await;
    h.send(Msg::CopyPrimary).await;
    assert_eq!(
        h.clipboard.copies(),
        vec![(
            "SECRET-FIXTURE-gh".to_owned(),
            true,
            Duration::from_secs(90)
        )]
    );
    assert!(!h.window_visible());
    assert_eq!(h.model.view.query, "");
    assert_eq!(h.model.usage.recency(&key("gh")), Some(h.now()));
    assert!(h.persisted() >= 1);
}

#[tokio::test]
async fn escape_hides_and_clears_query() {
    let mut h = opened().await;
    h.send(Msg::QueryChanged("bank".into())).await;
    h.send(Msg::Escape).await;
    assert!(!h.window_visible());
    h.send(Msg::Toggle).await;
    assert_eq!(h.model.view.query, "");
}

#[tokio::test]
async fn enter_with_no_results_does_nothing() {
    let mut h = opened().await;
    h.send(Msg::QueryChanged("zzzz".into())).await;
    h.send(Msg::CopyPrimary).await;
    assert!(h.clipboard.copies().is_empty());
    assert!(h.window_visible());
}

#[tokio::test]
async fn escape_cancels_slow_fetch() {
    let mut h = opened().await;
    h.backend.set_delay(Duration::from_millis(300));
    h.send(Msg::QueryChanged("bank".into())).await;
    h.send_no_wait(Msg::CopyPrimary);
    assert!(h.model.view.pending.is_some());
    h.send(Msg::Escape).await;
    assert!(h.model.view.pending.is_none());
    assert!(
        h.window_visible(),
        "escape with a pending fetch only cancels"
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    h.settle().await;
    assert!(h.clipboard.copies().is_empty());
    assert!(h.window_visible());
}

#[tokio::test]
async fn deleted_item_shows_notice_and_refreshes() {
    let mut h = opened().await;
    h.backend
        .fail_field(&key("gh"), "password", PassError::NotFound);
    let refreshes = h.backend.list_calls();
    h.send(Msg::QueryChanged("github".into())).await;
    h.send(Msg::CopyPrimary).await;
    assert!(h.clipboard.copies().is_empty());
    assert_eq!(
        h.model.view.notice.as_ref().map(|n| n.text.as_str()),
        Some("Item no longer exists")
    );
    assert_eq!(h.backend.list_calls(), refreshes + 1);
    assert!(h.window_visible());
}

#[tokio::test]
async fn empty_field_shows_notice() {
    let mut h = opened().await;
    h.send(Msg::QueryChanged("mail".into())).await;
    h.send(Msg::CopyPrimary).await;
    assert!(h.clipboard.copies().is_empty());
    assert_eq!(
        h.model.view.notice.as_ref().map(|n| n.text.as_str()),
        Some("This field is empty")
    );
}

#[tokio::test]
async fn copied_item_is_listed_first_on_empty_query() {
    let mut h = opened().await;
    assert_eq!(h.result_ids()[0], "bank");
    h.send(Msg::QueryChanged("github".into())).await;
    h.send(Msg::CopyPrimary).await;
    h.send(Msg::Toggle).await;
    assert_eq!(h.result_ids()[0], "gh");
}

#[tokio::test]
async fn stored_plain_value_is_copied_without_backend_call() {
    let mut h = Harness::new(FakeBackend::with_items(vec![{
        let mut i = summary("u", "Only user", ItemKind::Login);
        i.fields = vec![FieldRef::plain("username", "Username", "alice".into())];
        i
    }]));
    h.send(Msg::RefreshRequested).await;
    h.send(Msg::Show).await;
    h.send(Msg::CopyPrimary).await;
    assert_eq!(
        h.clipboard.copies(),
        vec![("alice".to_owned(), false, Duration::from_secs(90))]
    );
    assert_eq!(h.backend.field_calls(), 0);
}

#[tokio::test]
async fn clipboard_failure_is_reported() {
    let mut h = opened().await;
    h.clipboard.fail();
    h.send(Msg::QueryChanged("github".into())).await;
    h.send(Msg::CopyPrimary).await;
    assert_eq!(
        h.model.view.notice.as_ref().map(|n| n.text.as_str()),
        Some("Clipboard unavailable")
    );
}

#[tokio::test]
async fn pending_state_marks_the_row() {
    let mut h = opened().await;
    h.backend.set_delay(Duration::from_millis(200));
    h.send(Msg::QueryChanged("github".into())).await;
    h.send_no_wait(Msg::CopyPrimary);
    assert_eq!(
        h.model.view.pending.as_ref().map(|p| p.key.clone()),
        Some(key("gh"))
    );
    assert_eq!(h.model.view.mode, Mode::List);
    h.settle().await;
    assert!(h.model.view.pending.is_none());
    assert_eq!(h.clipboard.copies().len(), 1);
}

mod startup_from_cache {
    use super::*;
    use cosmic_pass::core::state::DataSource;
    use cosmic_pass::core::usage::UsageRecord;
    use cosmic_pass::model::{AccountId, CACHE_FORMAT_VERSION, CacheFile};

    fn cached(account: &str) -> CacheFile {
        CacheFile {
            format_version: CACHE_FORMAT_VERSION,
            account: AccountId(account.into()),
            fetched_at: 10,
            vaults: vec![],
            items: vec![
                login("gh", "GitHub", "octocat", "https://github.com"),
                login("bank", "Bank", "me", "https://bank.example"),
            ],
            usage: vec![UsageRecord {
                key: key("gh"),
                last_used: 99,
                count: 1,
            }],
        }
    }

    #[tokio::test]
    async fn cached_items_are_searchable_before_refresh() {
        let mut h = Harness::new(backend());
        h.send(Msg::CacheLoaded(Some(cached("account-1")))).await;
        assert_eq!(h.model.data.source, DataSource::DiskCache);
        assert!(h.model.data.stale);
        assert_eq!(h.model.data.fetched_at, None);
        h.send(Msg::QueryChanged("bank".into())).await;
        assert_eq!(h.result_ids(), ["bank"]);
        h.send(Msg::QueryChanged(String::new())).await;
        assert_eq!(h.result_ids()[0], "gh", "usage restored from cache");
    }

    #[tokio::test]
    async fn fresh_data_is_not_replaced_by_cache() {
        let mut h = opened().await;
        h.send(Msg::CacheLoaded(Some(cached("account-1")))).await;
        assert_eq!(h.model.data.items.len(), 3);
        assert_eq!(h.model.data.source, DataSource::Memory);
    }

    #[tokio::test]
    async fn cache_of_other_account_is_dropped() {
        let mut h = Harness::new(backend());
        h.send(Msg::CacheLoaded(Some(cached("someone-else")))).await;
        assert_eq!(h.model.data.items.len(), 2);
        let deletions = h.cache_deletions();
        h.send(Msg::SessionProbed(Ok(AccountId("account-1".into()))))
            .await;
        assert_eq!(h.cache_deletions(), deletions + 1);
        assert_eq!(
            h.model.data.items.len(),
            3,
            "fresh listing replaces the dropped cache"
        );
    }

    #[tokio::test]
    async fn refresh_persists_snapshot() {
        let mut h = Harness::new(backend());
        h.send(Msg::SessionProbed(Ok(AccountId("account-1".into()))))
            .await;
        assert!(h.persisted() >= 1);
        let snapshot = h.model.cache_snapshot().unwrap();
        assert_eq!(snapshot.account, AccountId("account-1".into()));
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(snapshot.fetched_at, h.now());
    }

    #[tokio::test]
    async fn no_snapshot_without_account() {
        let mut h = opened().await;
        h.send(Msg::CopyUsername).await;
        assert!(h.model.cache_snapshot().is_none());
    }
}

#[tokio::test]
async fn typing_stays_fast_during_slow_refresh() {
    let mut h = opened().await;
    h.backend.set_delay(Duration::from_secs(3));
    h.send_no_wait(Msg::RefreshRequested);
    assert!(h.model.data.refreshing);
    let mut query = String::new();
    for i in 0..200 {
        query.push(if i % 2 == 0 { 'b' } else { 'a' });
        let start = std::time::Instant::now();
        h.send_no_wait(Msg::QueryChanged(query.clone()));
        assert!(
            start.elapsed() < Duration::from_millis(5),
            "keystroke {i} took {:?}",
            start.elapsed()
        );
    }
    assert_eq!(h.model.view.query, query, "every keystroke is reflected");
    assert!(h.model.data.refreshing, "refresh still running");
}
