//! Runs `pass-cli` subprocesses with timeouts, cancellation, and a concurrency limit.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use super::error::PassError;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Successful process output. Stdout may contain secrets and is wiped on drop.
pub struct Output {
    pub stdout: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Output {{ stdout: <{} bytes redacted> }}",
            self.stdout.len()
        )
    }
}

pub trait CommandRunner: Send + Sync {
    /// Runs `pass-cli` with `args`. Fails with [`PassError::Timeout`] after `timeout` and with
    /// [`PassError::Cancelled`] when `cancel` fires; the process group is killed in both cases.
    fn run(
        &self,
        args: Vec<String>,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<Output, PassError>>;

    /// Runs `pass-cli` with `args`, forwarding every stdout and stderr line to `lines`.
    fn run_streaming(
        &self,
        args: Vec<String>,
        timeout: Duration,
        lines: tokio::sync::mpsc::Sender<String>,
    ) -> BoxFuture<'_, Result<(), PassError>>;
}

pub struct TokioRunner {
    bin: PathBuf,
    env: Vec<(String, String)>,
    permits: std::sync::Arc<tokio::sync::Semaphore>,
}

impl TokioRunner {
    pub const MAX_CONCURRENT: usize = 4;

    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self {
            bin: bin.into(),
            env: Vec::new(),
            permits: std::sync::Arc::new(tokio::sync::Semaphore::new(Self::MAX_CONCURRENT)),
        }
    }

    /// Uses `$COSMIC_PASS_CLI`, else `pass-cli` from `PATH`.
    pub fn from_env() -> Self {
        Self::new(std::env::var_os("COSMIC_PASS_CLI").unwrap_or_else(|| "pass-cli".into()))
    }

    /// Adds environment variables for the child (used by tests to drive the fake CLI).
    pub fn with_env(mut self, vars: Vec<(String, String)>) -> Self {
        self.env.extend(vars);
        self
    }

    async fn run_inner(
        &self,
        args: Vec<String>,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<Output, PassError> {
        let _permit = tokio::select! {
            permit = self.permits.acquire() => permit.map_err(|_| PassError::Cancelled)?,
            () = cancel.cancelled() => return Err(PassError::Cancelled),
        };
        let child = tokio::process::Command::new(&self.bin)
            .args(&args)
            .env("PASS_LOG_LEVEL", "off")
            .env("PROTON_PASS_NO_UPDATE_CHECK", "1")
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .process_group(0)
            .spawn()
            .map_err(|e| classify_spawn(e.kind()))?;
        let group = ProcessGroup::new(child.id());

        let output = tokio::select! {
            result = tokio::time::timeout(timeout, child.wait_with_output()) => match result {
                Ok(output) => output.map_err(|e| PassError::Cli { message: e.to_string() })?,
                Err(_) => return Err(PassError::Timeout),
            },
            () = cancel.cancelled() => return Err(PassError::Cancelled),
        };
        group.disarm();

        if output.status.success() {
            Ok(Output {
                stdout: Zeroizing::new(output.stdout),
            })
        } else {
            drop(Zeroizing::new(output.stdout));
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(super::error::classify(&super::error::Failure {
                stderr: &stderr,
                ..Default::default()
            }))
        }
    }
}

impl TokioRunner {
    async fn run_streaming_inner(
        &self,
        args: Vec<String>,
        timeout: Duration,
        lines: tokio::sync::mpsc::Sender<String>,
    ) -> Result<(), PassError> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut child = tokio::process::Command::new(&self.bin)
            .args(&args)
            .env("PASS_LOG_LEVEL", "off")
            .env("PROTON_PASS_NO_UPDATE_CHECK", "1")
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .process_group(0)
            .spawn()
            .map_err(|e| classify_spawn(e.kind()))?;
        let group = ProcessGroup::new(child.id());
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(PassError::Cli {
                message: "no output pipes".into(),
            });
        };

        let work = async {
            let mut out = BufReader::new(stdout).lines();
            let mut err = BufReader::new(stderr).lines();
            let mut err_text = String::new();
            let (mut out_done, mut err_done) = (false, false);
            while !(out_done && err_done) {
                tokio::select! {
                    line = out.next_line(), if !out_done => match line {
                        Ok(Some(l)) => { let _ = lines.send(l).await; }
                        _ => out_done = true,
                    },
                    line = err.next_line(), if !err_done => match line {
                        Ok(Some(l)) => {
                            err_text.push_str(&l);
                            err_text.push('\n');
                            let _ = lines.send(l).await;
                        }
                        _ => err_done = true,
                    },
                }
            }
            let status = child.wait().await.map_err(|e| PassError::Cli {
                message: e.to_string(),
            })?;
            Ok::<_, PassError>((status, err_text))
        };
        let (status, err_text) = tokio::time::timeout(timeout, work)
            .await
            .map_err(|_| PassError::Timeout)??;
        group.disarm();
        if status.success() {
            Ok(())
        } else {
            Err(super::error::classify(&super::error::Failure {
                stderr: &err_text,
                ..Default::default()
            }))
        }
    }
}

impl CommandRunner for TokioRunner {
    fn run(
        &self,
        args: Vec<String>,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<Output, PassError>> {
        Box::pin(self.run_inner(args, timeout, cancel))
    }

    fn run_streaming(
        &self,
        args: Vec<String>,
        timeout: Duration,
        lines: tokio::sync::mpsc::Sender<String>,
    ) -> BoxFuture<'_, Result<(), PassError>> {
        Box::pin(self.run_streaming_inner(args, timeout, lines))
    }
}

fn classify_spawn(kind: std::io::ErrorKind) -> PassError {
    super::error::classify(&super::error::Failure {
        spawn_error: Some(kind),
        ..Default::default()
    })
}

/// Kills the child's whole process group when dropped, unless disarmed after a normal exit.
/// `kill_on_drop` only reaches the direct child; this also reaches its descendants.
struct ProcessGroup(Option<rustix::process::Pid>);

impl ProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self(
            pid.and_then(|p| i32::try_from(p).ok())
                .and_then(rustix::process::Pid::from_raw),
        )
    }

    fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            // The group may already be gone; nothing useful to do on failure.
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}
