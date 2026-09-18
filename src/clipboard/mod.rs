//! Clipboard access through a helper process that clears secrets after a timeout.
//!
//! Each copy runs `cosmic-pass clipboard-serve`, which owns the selection until another
//! client replaces it (the helper exits on its own) or until it is killed when the timeout
//! expires (the compositor then clears the selection). A value is therefore only cleared
//! while it is still the current selection (FR-015).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::pass::runner::BoxFuture;

pub mod serve;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClipboardError {
    #[error("the compositor does not offer clipboard access")]
    Unavailable,
    #[error("clipboard helper failed: {0}")]
    Helper(String),
}

pub trait Clipboard: Send + Sync {
    /// Copies `value`. Secret values are cleared after `clear_after` if still selected.
    fn copy(
        &self,
        value: SecretString,
        secret: bool,
        clear_after: Duration,
    ) -> BoxFuture<'_, Result<(), ClipboardError>>;
}

/// How a copy job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobEnd {
    /// Another client took the selection; nothing to clear.
    OwnershipLost,
    /// The timeout expired and the helper was killed, clearing the selection.
    Cleared,
    /// A newer copy replaced this job.
    Replaced,
}

struct Job {
    cancel: CancellationToken,
    end: watch::Receiver<Option<JobEnd>>,
}

pub struct HelperClipboard {
    program: PathBuf,
    args: Vec<OsString>,
    env: Vec<(String, String)>,
    current: Mutex<Option<Job>>,
}

impl HelperClipboard {
    /// Runs `program args... --timeout <secs> [--secret]`.
    pub fn new(program: impl Into<PathBuf>, args: Vec<OsString>) -> Self {
        Self {
            program: program.into(),
            args,
            env: Vec::new(),
            current: Mutex::new(None),
        }
    }

    /// The running binary's own `clipboard-serve` subcommand, or
    /// `$COSMIC_PASS_CLIPBOARD_HELPER` when set (tests).
    pub fn from_env() -> std::io::Result<Self> {
        if let Some(helper) = std::env::var_os("COSMIC_PASS_CLIPBOARD_HELPER") {
            return Ok(Self::new(helper, Vec::new()));
        }
        Ok(Self::new(
            std::env::current_exe()?,
            vec!["clipboard-serve".into()],
        ))
    }

    pub fn with_env(mut self, vars: Vec<(String, String)>) -> Self {
        self.env.extend(vars);
        self
    }

    /// Waits for the current job to end and reports how. `None` if there is no job.
    pub async fn wait_current(&self) -> Option<JobEnd> {
        let mut end = self.lock().as_ref()?.end.clone();
        end.wait_for(Option::is_some).await.ok().and_then(|v| *v)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Job>> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn copy_inner(
        &self,
        value: SecretString,
        secret: bool,
        clear_after: Duration,
    ) -> Result<(), ClipboardError> {
        if let Some(previous) = self.lock().take() {
            previous.cancel.cancel();
        }

        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args)
            .arg("--timeout")
            .arg(clear_after.as_secs().max(1).to_string());
        if secret {
            cmd.arg("--secret");
        }
        let mut child = cmd
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ClipboardError::Helper(e.to_string()))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ClipboardError::Helper("no stdin".into()))?;
        let written = stdin.write_all(value.expose_secret().as_bytes()).await;
        drop(value);
        drop(stdin);
        written.map_err(|e| ClipboardError::Helper(e.to_string()))?;
        wait_ready(&mut child).await?;

        let cancel = CancellationToken::new();
        let (tx, rx) = watch::channel(None);
        let job_cancel = cancel.clone();
        tokio::spawn(async move {
            let deadline = async {
                if secret {
                    tokio::time::sleep(clear_after).await;
                } else {
                    std::future::pending::<()>().await;
                }
            };
            let end = tokio::select! {
                _ = child.wait() => JobEnd::OwnershipLost,
                () = deadline => {
                    let _ = child.kill().await;
                    JobEnd::Cleared
                }
                () = job_cancel.cancelled() => {
                    let _ = child.kill().await;
                    JobEnd::Replaced
                }
            };
            let _ = tx.send(Some(end));
        });
        *self.lock() = Some(Job { cancel, end: rx });
        Ok(())
    }
}

impl Clipboard for HelperClipboard {
    fn copy(
        &self,
        value: SecretString,
        secret: bool,
        clear_after: Duration,
    ) -> BoxFuture<'_, Result<(), ClipboardError>> {
        Box::pin(self.copy_inner(value, secret, clear_after))
    }
}

impl Drop for HelperClipboard {
    fn drop(&mut self) {
        if let Some(job) = self.lock().take() {
            job.cancel.cancel();
        }
    }
}

/// Waits for the helper's `ready` line, which it prints once it owns the selection.
async fn wait_ready(child: &mut tokio::process::Child) -> Result<(), ClipboardError> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ClipboardError::Helper("no stdout".into()))?;
    let mut line = String::new();
    let read = tokio::time::timeout(
        READY_TIMEOUT,
        tokio::io::BufReader::new(stdout).read_line(&mut line),
    )
    .await;
    if matches!(read, Ok(Ok(_))) && line.trim() == serve::READY {
        return Ok(());
    }
    let _ = child.start_kill();
    match child.wait().await.ok().and_then(|s| s.code()) {
        Some(serve::EXIT_UNAVAILABLE) => Err(ClipboardError::Unavailable),
        code => Err(ClipboardError::Helper(format!(
            "helper did not become ready (exit {code:?})"
        ))),
    }
}

const READY_TIMEOUT: Duration = Duration::from_secs(3);

pub fn shared(clipboard: HelperClipboard) -> Arc<dyn Clipboard> {
    Arc::new(clipboard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Instant;

    struct Env {
        dir: tempfile::TempDir,
    }

    impl Env {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, name: &str) -> String {
            self.dir.path().join(name).display().to_string()
        }

        fn clipboard(&self, extra: &[(&str, &str)]) -> HelperClipboard {
            let script =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-clipboard-serve");
            let mut vars = vec![
                ("FAKE_CLIP_OUT".to_owned(), self.path("out")),
                ("FAKE_CLIP_ARGV".to_owned(), self.path("argv")),
                ("FAKE_CLIP_PID".to_owned(), self.path("pid")),
            ];
            vars.extend(
                extra
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
            );
            HelperClipboard::new(script, vec!["clipboard-serve".into()]).with_env(vars)
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.path(name)).unwrap_or_default()
        }

        fn pid(&self) -> i32 {
            self.read("pid").trim().parse().unwrap()
        }
    }

    fn alive(pid: i32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
            !s.rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z')
        })
    }

    const SHORT: Duration = Duration::from_millis(300);

    #[tokio::test]
    async fn value_goes_through_stdin_only() {
        let env = Env::new();
        let cb = env.clipboard(&[]);
        cb.copy(SecretString::from("SECRET-FIXTURE-x"), true, SHORT)
            .await
            .unwrap();
        assert_eq!(env.read("out"), "SECRET-FIXTURE-x");
        let argv = env.read("argv");
        assert_eq!(argv.trim(), "clipboard-serve --timeout 1 --secret");
        assert!(!argv.contains("SECRET-FIXTURE"));
        cb.wait_current().await;
    }

    #[tokio::test]
    async fn secret_is_cleared_at_timeout() {
        let env = Env::new();
        let cb = env.clipboard(&[]);
        let start = Instant::now();
        cb.copy(SecretString::from("s"), true, SHORT).await.unwrap();
        let pid = env.pid();
        assert!(alive(pid));
        assert_eq!(cb.wait_current().await, Some(JobEnd::Cleared));
        let elapsed = start.elapsed();
        assert!(
            elapsed >= SHORT && elapsed < SHORT + Duration::from_secs(1),
            "{elapsed:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!alive(pid));
    }

    #[tokio::test]
    async fn ownership_loss_ends_job_without_kill() {
        let env = Env::new();
        let cb = env.clipboard(&[("FAKE_CLIP_LOSE", "1")]);
        cb.copy(SecretString::from("s"), true, Duration::from_secs(30))
            .await
            .unwrap();
        let end = tokio::time::timeout(Duration::from_secs(2), cb.wait_current())
            .await
            .unwrap();
        assert_eq!(end, Some(JobEnd::OwnershipLost));
    }

    #[tokio::test]
    async fn new_copy_replaces_previous_job() {
        let env = Env::new();
        let cb = env.clipboard(&[]);
        cb.copy(SecretString::from("first"), true, Duration::from_secs(30))
            .await
            .unwrap();
        let first_pid = env.pid();
        let mut first_end = cb.lock().as_ref().unwrap().end.clone();
        cb.copy(SecretString::from("second"), true, Duration::from_secs(30))
            .await
            .unwrap();
        first_end.wait_for(Option::is_some).await.unwrap();
        assert_eq!(*first_end.borrow(), Some(JobEnd::Replaced));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!alive(first_pid));
        assert_eq!(env.read("out"), "second");
        drop(cb);
    }

    #[tokio::test]
    async fn non_secret_copy_has_no_timeout() {
        let env = Env::new();
        let cb = env.clipboard(&[]);
        cb.copy(SecretString::from("user"), false, SHORT)
            .await
            .unwrap();
        assert!(!env.read("argv").contains("--secret"));
        let waited = tokio::time::timeout(SHORT * 3, cb.wait_current()).await;
        assert!(waited.is_err(), "non-secret job must keep running");
        assert!(alive(env.pid()));
    }

    #[tokio::test]
    async fn unavailable_protocol_is_reported() {
        let env = Env::new();
        let code = serve::EXIT_UNAVAILABLE.to_string();
        let cb = env.clipboard(&[("FAKE_CLIP_EXIT", code.as_str())]);
        let err = cb
            .copy(SecretString::from("s"), true, SHORT)
            .await
            .unwrap_err();
        assert_eq!(err, ClipboardError::Unavailable);
    }

    #[tokio::test]
    async fn dropping_the_clipboard_kills_the_helper() {
        let env = Env::new();
        let cb = env.clipboard(&[]);
        cb.copy(SecretString::from("s"), false, SHORT)
            .await
            .unwrap();
        let pid = env.pid();
        drop(cb);
        for _ in 0..50 {
            if !alive(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("helper still alive");
    }
}
