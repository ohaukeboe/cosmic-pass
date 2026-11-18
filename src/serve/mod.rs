//! `cosmic-pass serve`: the reducer without a window, for another front end.
//!
//! Intents arrive as JSON lines on stdin; after every update that changes what a front end
//! would draw, one JSON snapshot line goes to stdout. Effects run through the same
//! [`runtime::execute`] the popup uses, so copying, revealing, the cache and sign-in behave
//! exactly as there. Contract: `specs/004-serve-mode/contracts/serve-protocol.md`.

pub mod protocol;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use zeroize::Zeroizing;

use crate::config::Preferences;
use crate::core::state::{Model, Msg};
use crate::pass::backend::PassCli;
use crate::pass::runner::TokioRunner;
use crate::runtime::{Deps, Step, execute};
use protocol::{Intent, Snapshot};

/// Rows a snapshot carries at most. The popup shows a dozen; a front end that scrolls wants
/// more, and 50 still keeps a snapshot line small.
pub const MAX_RESULTS: u16 = 50;

/// Entry point for the subcommand. `0` when stdin closes, `1` when stdout breaks.
pub fn main(clipboard_clear_secs: u32) -> ExitCode {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::error!("starting the runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let keys: Arc<dyn crate::cache::keystore::KeyStore> =
        Arc::new(crate::cache::keystore::Oo7KeyStore);
    let deps = Deps {
        backend: Arc::new(PassCli::new(Arc::new(TokioRunner::from_env()))),
        clipboard: crate::clipboard::from_env_or_none(),
        cache: Some(crate::cache::store::CacheStore::from_env(keys)),
        // The front end's own configuration owns the settings; never write cosmic-config.
        save_preferences: false,
    };
    let prefs = Preferences {
        clipboard_clear_secs,
        max_results: MAX_RESULTS,
        ..Preferences::default()
    };
    let input = BufReader::new(tokio::io::stdin());
    let result = runtime.block_on(run(input, tokio::io::stdout(), deps, prefs, unix_now));
    // Effects still running (a refresh, a cache write) are dropped with the runtime; a copy
    // in flight lives in the clipboard helper process and still clears on time.
    runtime.shutdown_background();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("writing a snapshot: {e}");
            ExitCode::FAILURE
        }
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The loop, generic over its pipes and clock so tests drive it in-process. Returns when
/// `input` ends, or with the error when `output` cannot be written.
pub async fn run<R, W, C>(
    input: R,
    mut output: W,
    deps: Deps,
    prefs: Preferences,
    clock: C,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    C: Fn() -> i64,
{
    let mut model = Model::new(prefs);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut lines = input.lines();
    let mut last: Option<SecretString> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    dispatch(&mut model, Msg::Startup, clock(), &deps, &tx);
    write_if_changed(&model, clock(), &mut output, &mut last).await?;
    loop {
        let msg = tokio::select! {
            line = lines.next_line() => match line? {
                None => return Ok(()),
                Some(line) if line.trim().is_empty() => continue,
                Some(line) => match serde_json::from_str::<Intent>(&line) {
                    Ok(intent) => Msg::from(intent),
                    Err(e) => {
                        // Never the line itself: a query carries what the user typed.
                        tracing::warn!(
                            line = e.line(),
                            column = e.column(),
                            "skipping unparseable intent: {:?}",
                            e.classify()
                        );
                        continue;
                    }
                },
            },
            Some(msg) = rx.recv() => msg,
            _ = tick.tick() => {
                // The app's condition for its clock subscription (`app/mod.rs`): only a shown
                // one-time code needs one.
                if !(model.view.visible && model.revealed_totp().is_some()) {
                    continue;
                }
                Msg::Tick(clock())
            }
        };
        dispatch(&mut model, msg, clock(), &deps, &tx);
        write_if_changed(&model, clock(), &mut output, &mut last).await?;
    }
}

/// Updates the model and starts its effects; their messages come back through `tx`.
fn dispatch(model: &mut Model, msg: Msg, now: i64, deps: &Deps, tx: &mpsc::UnboundedSender<Msg>) {
    for effect in model.update(msg, now) {
        match execute(effect, deps, model) {
            Step::Future(future) => {
                let tx = tx.clone();
                tokio::spawn(async move {
                    if let Some(msg) = future.await {
                        let _ = tx.send(msg);
                    }
                });
            }
            Step::Stream(stream) => {
                let tx = tx.clone();
                tokio::spawn(async move {
                    use futures::StreamExt;
                    let mut stream = stream;
                    while let Some(msg) = stream.next().await {
                        if tx.send(msg).is_err() {
                            break;
                        }
                    }
                });
            }
            // `visible` is read from the model; there is no window to show or hide.
            Step::ShowWindow | Step::HideWindow | Step::Unhandled(_) => {}
        }
    }
}

/// Writes the snapshot when it differs from the last one written. The copy kept for the
/// comparison is a [`SecretString`], since a snapshot can hold a revealed secret.
async fn write_if_changed<W: AsyncWrite + Unpin>(
    model: &Model,
    now: i64,
    output: &mut W,
    last: &mut Option<SecretString>,
) -> std::io::Result<()> {
    let line = Zeroizing::new(
        serde_json::to_string(&Snapshot::of(model, now)).map_err(std::io::Error::other)?,
    );
    if last
        .as_ref()
        .is_some_and(|l| l.expose_secret() == line.as_str())
    {
        return Ok(());
    }
    output.write_all(line.as_bytes()).await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    *last = Some(SecretString::from(line.as_str()));
    Ok(())
}
