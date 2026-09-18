//! Executes the IO side of reducer effects. Shared by the app and the test harness.

use std::sync::Arc;
use std::time::Duration;

use crate::cache::store::CacheStore;
use crate::clipboard::Clipboard;
use crate::config::Preferences;
use crate::core::effects::Effect;
use crate::core::state::{Model, Msg};
use crate::pass::backend::PassBackend;
use crate::pass::error::PassError;
use crate::pass::runner::BoxFuture;

#[derive(Clone)]
pub struct Deps {
    pub backend: Arc<dyn PassBackend>,
    pub clipboard: Arc<dyn Clipboard>,
    /// `None` disables the on-disk cache.
    pub cache: Option<CacheStore>,
    /// Whether `SavePreferences` writes to cosmic-config (disabled in tests).
    pub save_preferences: bool,
}

/// What the caller must do for one effect.
pub enum Step {
    ShowWindow,
    HideWindow,
    /// Run this future; feed its message back into the reducer.
    Future(BoxFuture<'static, Option<Msg>>),
    /// Feed every message of this stream back into the reducer.
    Stream(futures::stream::BoxStream<'static, Msg>),
    /// Nothing to run (for example `Persist` without a cache or snapshot).
    Unhandled(Effect),
}

pub fn execute(effect: Effect, deps: &Deps, model: &Model) -> Step {
    let prefs = &model.prefs;
    match effect {
        Effect::ShowWindow => Step::ShowWindow,
        Effect::HideWindow => Step::HideWindow,
        Effect::Refresh => {
            let backend = deps.backend.clone();
            future(async move {
                let started = std::time::Instant::now();
                let result = backend.list_all().await;
                match result {
                    Ok(listing) => {
                        tracing::debug!(
                            items = listing.items.len(),
                            "listed items in {:?}",
                            started.elapsed()
                        );
                        Some(Msg::DataLoaded(listing))
                    }
                    Err(e) => {
                        tracing::warn!("refresh failed after {:?}: {e}", started.elapsed());
                        Some(Msg::RefreshFailed(e))
                    }
                }
            })
        }
        Effect::FetchAndCopy {
            key, field, cancel, ..
        } => {
            let backend = deps.backend.clone();
            future(async move {
                let result = backend.get_field(key.clone(), field, cancel).await;
                Some(Msg::CopyFetched { key, result })
            })
        }
        Effect::FetchTotpAndCopy { key, field, cancel } => {
            let backend = deps.backend.clone();
            future(async move {
                let result = backend
                    .totp(key.clone(), cancel)
                    .await
                    .and_then(|mut codes| pick_totp(&mut codes, &field));
                Some(Msg::CopyFetched { key, result })
            })
        }
        Effect::Copy { value, secret } => {
            let clipboard = deps.clipboard.clone();
            let clear_after = Duration::from_secs(u64::from(prefs.clipboard_clear_secs));
            future(async move {
                let result = clipboard.copy(value, secret, clear_after).await;
                if let Err(e) = &result {
                    tracing::warn!("copy failed: {e}");
                }
                Some(Msg::CopyFinished(result.map_err(|e| e.to_string())))
            })
        }
        Effect::ProbeSession => {
            let backend = deps.backend.clone();
            future(async move { Some(Msg::SessionProbed(backend.account().await)) })
        }
        Effect::StartLogin => Step::Stream(login_stream(deps.backend.clone())),
        Effect::FetchReveal { key, field, cancel } => {
            let backend = deps.backend.clone();
            future(async move {
                let result = backend.get_field(key.clone(), field, cancel).await;
                Some(Msg::RevealFetched { key, result })
            })
        }
        Effect::FetchTotp { key, cancel } => {
            let backend = deps.backend.clone();
            future(async move {
                let result = backend.totp(key.clone(), cancel).await;
                Some(Msg::TotpFetched { key, result })
            })
        }
        Effect::LoadCache => {
            let cache = deps.cache.clone();
            future(async move {
                let file = match cache {
                    Some(cache) => cache.load().await,
                    None => None,
                };
                Some(Msg::CacheLoaded(file))
            })
        }
        Effect::Persist => match (deps.cache.clone(), model.cache_snapshot()) {
            (Some(cache), Some(file)) => future(async move {
                cache.save_debounced(file);
                None
            }),
            _ => Step::Unhandled(Effect::Persist),
        },
        Effect::DeleteCache => {
            let cache = deps.cache.clone();
            future(async move {
                if let Some(cache) = cache {
                    cache.delete_all().await;
                }
                None
            })
        }
        Effect::SavePreferences(prefs) => {
            if !deps.save_preferences {
                return Step::Unhandled(Effect::SavePreferences(prefs));
            }
            future(async move {
                let written = tokio::task::spawn_blocking(move || {
                    use cosmic::cosmic_config::CosmicConfigEntry;
                    let config = cosmic::cosmic_config::Config::new(
                        crate::config::CONFIG_ID,
                        Preferences::VERSION,
                    )?;
                    prefs.write_entry(&config)
                })
                .await;
                match written {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => {
                        tracing::warn!("saving preferences: {e}");
                        None
                    }
                    Err(e) => {
                        tracing::warn!("saving preferences: {e}");
                        None
                    }
                }
            })
        }
        Effect::ExpireNotice { id, after } => future(async move {
            tokio::time::sleep(after).await;
            Some(Msg::NoticeExpired(id))
        }),
    }
}

/// `LoginLine` for each output line, then `LoginFinished`.
fn login_stream(backend: Arc<dyn PassBackend>) -> futures::stream::BoxStream<'static, Msg> {
    use futures::StreamExt;
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let login = tokio::spawn(async move { backend.login(tx).await });
    let lines = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|line| (Msg::LoginLine(line), rx))
    });
    let finished = futures::stream::once(async move {
        let result = login.await.unwrap_or_else(|e| {
            Err(PassError::Cli {
                message: e.to_string(),
            })
        });
        Msg::LoginFinished(result)
    });
    lines.chain(finished).boxed()
}

fn future(f: impl Future<Output = Option<Msg>> + Send + 'static) -> Step {
    Step::Future(Box::pin(f))
}

/// The code for `field`. The login code is reported as `totp_uri` (with `totp` as an alias).
fn pick_totp(
    codes: &mut std::collections::BTreeMap<String, secrecy::SecretString>,
    field: &str,
) -> Result<secrecy::SecretString, PassError> {
    let alias = if field == "totp_uri" { "totp" } else { field };
    codes
        .remove(field)
        .or_else(|| codes.remove(alias))
        .ok_or(PassError::FieldMissing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::{ExposeSecret, SecretString};
    use std::collections::BTreeMap;

    fn codes(pairs: &[(&str, &str)]) -> BTreeMap<String, SecretString> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), SecretString::from(*v)))
            .collect()
    }

    #[test]
    fn totp_field_lookup() {
        let mut c = codes(&[("totp", "1"), ("Backup", "2")]);
        assert_eq!(pick_totp(&mut c, "totp_uri").unwrap().expose_secret(), "1");
        let mut c = codes(&[("totp_uri", "3"), ("totp", "1")]);
        assert_eq!(pick_totp(&mut c, "totp_uri").unwrap().expose_secret(), "3");
        let mut c = codes(&[("Backup", "2")]);
        assert_eq!(pick_totp(&mut c, "Backup").unwrap().expose_secret(), "2");
        assert_eq!(
            pick_totp(&mut codes(&[]), "totp_uri").unwrap_err(),
            PassError::FieldMissing
        );
    }
}
