//! Resolves the real `pass-cli` and runs it without touching the developer's own session.
//!
//! The suites that use this drive the binary upstream actually ships, not
//! `tests/fixtures/fake-pass-cli`. See
//! `specs/003-pass-cli-contract-tests/contracts/pass-cli-test-harness.md` for the contract this
//! module implements.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cosmic_pass::core::version::TESTED_MIN;
use cosmic_pass::model::CliVersion;
use cosmic_pass::pass::error::{Failure, PassError, classify};
use cosmic_pass::pass::parse::parse_version;
use cosmic_pass::pass::runner::TokioRunner;
use sha2::{Digest, Sha256};

/// Env var naming the binary under test. The same one `TokioRunner::from_env` reads, so the app
/// and its tests can never disagree about which `pass-cli` is being exercised.
const BIN_VAR: &str = "COSMIC_PASS_CLI";
/// Set by the flake devShell. Turns "no `pass-cli` found" from a skip into a failure, so a
/// broken flake pin cannot silently disable the whole feature.
const REQUIRE_VAR: &str = "COSMIC_PASS_REQUIRE_CLI";
/// Opt-in for the live suite. Deliberately separate from `#[ignore]`: `--ignored` is a blunt,
/// commonly used flag, and nobody should drive their real vault by running it.
pub const LIVE_VAR: &str = "COSMIC_PASS_LIVE";

const DEFAULT_BIN: &str = "pass-cli";

// -------------------------------------------------------------------------------------------
// Discovery

/// The binary a run resolved, plus the version it reports. One per test binary.
#[derive(Debug)]
pub struct RealCli {
    pub path: PathBuf,
    pub version: CliVersion,
}

/// What discovery produced. The skip-versus-fail rule as data, rather than as control flow
/// repeated in every test.
#[derive(Debug)]
pub enum Resolution {
    Found(RealCli),
    /// Nothing resolved and the binary is not marked required.
    Skip {
        looked_for: String,
    },
    /// Nothing resolved while the binary is required, or the version is unusable.
    Fail {
        reason: String,
    },
}

/// Resolves the binary once per test binary, so the `--version` banner is parsed once rather
/// than once per test.
pub fn resolve() -> &'static Resolution {
    static RESOLVED: OnceLock<Resolution> = OnceLock::new();
    RESOLVED.get_or_init(resolve_uncached)
}

fn resolve_uncached() -> Resolution {
    let required = std::env::var_os(REQUIRE_VAR).is_some_and(|v| v == "1");
    let (path, looked_for) = match std::env::var_os(BIN_VAR) {
        Some(explicit) => {
            let path = PathBuf::from(&explicit);
            let describe = format!("{BIN_VAR}={}", path.display());
            (path.exists().then_some(path), describe)
        }
        None => (
            on_path(DEFAULT_BIN),
            format!("no {DEFAULT_BIN} on PATH and {BIN_VAR} unset"),
        ),
    };
    let Some(path) = path else {
        return if required {
            Resolution::Fail {
                reason: format!("{looked_for}, and {REQUIRE_VAR}=1 marks it required"),
            }
        } else {
            Resolution::Skip { looked_for }
        };
    };

    let version = match probe_version(&path) {
        Ok(version) => version,
        Err(reason) => return Resolution::Fail { reason },
    };
    if version < TESTED_MIN {
        return Resolution::Fail {
            reason: format!(
                "{} reports {version}, below the tested floor {TESTED_MIN}; \
                 the contract suite would be asserting against a pass-cli the app itself warns about",
                path.display()
            ),
        };
    }
    Resolution::Found(RealCli { path, version })
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Reads the version banner through the app's own parser, in a throwaway home so the probe
/// cannot create anything in the developer's profile.
fn probe_version(path: &Path) -> Result<CliVersion, String> {
    let home = IsolatedEnv::stateless();
    // Through `try_capture`, so the very first thing either suite runs already carries the
    // timeout every other probe does: a `pass-cli` that hangs on `--version` must fail
    // resolution, not wedge the test binary before a single test starts (FR-013).
    let out = RawOutput::try_capture(path, &["--version"], home.vars(), PROBE_TIMEOUT)?;
    if out.status != Some(0) {
        return Err(format!(
            "{} --version exited with {:?}",
            path.display(),
            out.status
        ));
    }
    parse_version(out.stdout.as_bytes())
        .map_err(|_| format!("{} --version printed no parseable version", path.display()))
}

/// The resolved binary, or `None` after printing the fixed skip line. Fails the test when the
/// binary is required but missing, or unusable.
///
/// Shaped as an `Option` rather than a macro so a test reads
/// `let Some(cli) = support::real_cli::or_skip() else { return };` -- ordinary control flow with
/// no macro export to reason about.
pub fn or_skip() -> Option<&'static RealCli> {
    match resolve() {
        Resolution::Found(cli) => Some(cli),
        Resolution::Skip { looked_for } => {
            // Fixed `SKIP <binary>: <reason>` shape, so it is greppable and visible in
            // `just check` output.
            eprintln!("SKIP {}: {looked_for}", env!("CARGO_CRATE_NAME"));
            None
        }
        Resolution::Fail { reason } => panic!("{reason}"),
    }
}

/// The resolved binary for a live test, or `None` after printing the skip line when the live
/// opt-in is absent.
pub fn or_skip_live() -> Option<&'static RealCli> {
    if std::env::var_os(LIVE_VAR).is_none_or(|v| v != "1") {
        eprintln!(
            "SKIP {}: set {LIVE_VAR}=1 (see `just test-live`)",
            env!("CARGO_CRATE_NAME")
        );
        return None;
    }
    or_skip()
}

impl RealCli {
    /// A runner with the production environment: what the app itself spawns, keyring included.
    /// Used by the live suite, which is the only place `PROTON_PASS_LINUX_KEYRING=dbus` can be
    /// exercised.
    pub fn runner(&self) -> TokioRunner {
        TokioRunner::new(&self.path)
    }

    /// Runs the binary with the production environment and hands back the raw result.
    pub fn raw(&self, args: &[&str]) -> RawOutput {
        RawOutput::capture(&self.path, args, Vec::new())
    }
}

// -------------------------------------------------------------------------------------------
// Isolation

/// A throwaway home for a contract probe.
///
/// The on-disk store is easy: `pass-cli` keeps it under `XDG_DATA_HOME`, so pointing that at a
/// temp directory is enough. The keyring is the real risk. Production sets
/// `PROTON_PASS_LINUX_KEYRING=dbus` (`src/pass/runner.rs`), which puts the session key in the
/// Secret Service -- a bus, not a directory, that a temp home does not isolate. So a probe
/// overrides that back to the upstream default and points the bus address at nothing.
///
/// None of this is trusted: every contract probe asserts it ended up unauthenticated, which is
/// what actually proves the isolation held.
pub struct IsolatedEnv {
    dir: PathBuf,
    /// Kept alive so a stateless home is removed on drop. `None` for a labelled home, which is
    /// meant to survive the run.
    _temp: Option<tempfile::TempDir>,
}

impl IsolatedEnv {
    /// A home at a path derived from `label`, wiped clean, and reused by every later run of the
    /// same probe.
    ///
    /// The path is stable on purpose, and a random `tempfile::tempdir()` would be a bug. The
    /// store `pass-cli` creates under `XDG_DATA_HOME` is encrypted with a key it keeps in the
    /// kernel keyring under `keyring:cli-local-key:<sha256 of the store path>@ProtonPassCLI`,
    /// and that key is `perm`: it lives in the per-uid persistent keyring, which no `HOME`
    /// override touches and which `logout --force` does not clear. A fresh random path every
    /// run therefore meant a fresh permanent key every run -- six per suite run, against a
    /// 200-key, 20000-byte quota shared with everything else the developer runs. It filled, and
    /// every probe then failed with `Error accessing keyring: Platform failure: QuotaExceeded`.
    ///
    /// A stable path makes the description stable, so the key is created once and reused
    /// forever after. Wiping the directory does not orphan it: the key is addressed by path,
    /// not by content, so the next run finds it again (asserted in
    /// `isolation::a_probe_reuses_one_kernel_key`).
    ///
    /// `label` must be unique per test. Two tests sharing one would share a directory, and
    /// nextest runs them concurrently.
    pub fn new(label: &str) -> Self {
        let dir = probe_home_root().join(label);
        // Wiped rather than merely created: a probe must not inherit whatever the last run
        // left, which is what a `TempDir` used to give for free.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("creating the probe home {}: {e}", dir.display()));
        Self { dir, _temp: None }
    }

    /// A throwaway home for a probe that runs only `--help` or `--version`.
    ///
    /// Those two touch neither the store nor the keyring -- measured on 2.3.3: zero files
    /// written, zero keys created, which `isolation::help_and_version_touch_no_store` asserts
    /// -- so they need no stable path, and a random one keeps them from contending for a
    /// directory with the many test processes nextest runs at once.
    pub fn stateless() -> Self {
        let temp = tempfile::tempdir().expect("a temp dir");
        Self {
            dir: temp.path().to_owned(),
            _temp: Some(temp),
        }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// The store whose path `pass-cli` hashes into its kernel-keyring key description.
    fn store(&self) -> PathBuf {
        self.dir.join("share/proton-pass-cli/.session")
    }

    /// The description `pass-cli` gives this home's kernel-keyring key.
    ///
    /// Reproduced rather than observed, so a probe can name its own key exactly instead of
    /// diffing `/proc/keys` against whatever a concurrent test is doing. Verified against 2.3.3.
    pub fn kernel_key_description(&self) -> String {
        let digest = Sha256::digest(self.store().display().to_string().as_bytes());
        format!("keyring:cli-local-key:{digest:x}@ProtonPassCLI")
    }

    /// How many kernel keys carry this home's description. The guarantee is that it never
    /// exceeds one, however often the suite runs.
    ///
    /// `/proc/keys` lists every key owned by this user; `None` means the kernel does not expose
    /// it, which is the only case where the count cannot be checked.
    pub fn kernel_keys_for_this_home(&self) -> Option<usize> {
        let keys = std::fs::read_to_string("/proc/keys").ok()?;
        let wanted = self.kernel_key_description();
        Some(keys.lines().filter(|l| l.contains(&wanted)).count())
    }

    /// Environment for a child process. Applied *after* the three variables `TokioRunner` sets
    /// itself, so the keyring override here wins.
    pub fn vars(&self) -> Vec<(String, String)> {
        let at = |name: &str| self.dir.join(name).display().to_string();
        vec![
            ("HOME".into(), self.dir.display().to_string()),
            ("XDG_DATA_HOME".into(), at("share")),
            ("XDG_CONFIG_HOME".into(), at("config")),
            ("XDG_STATE_HOME".into(), at("state")),
            ("XDG_CACHE_HOME".into(), at("cache")),
            // Not `dbus`: keep the probe away from the Secret Service where a real session key
            // lives. `kernel` is upstream's own default.
            ("PROTON_PASS_LINUX_KEYRING".into(), "kernel".into()),
            // `TokioRunner` has no way to unset an inherited variable, and growing a production
            // API for one test would cost more than it buys. An unroutable address reaches
            // nothing just as well.
            (
                "DBUS_SESSION_BUS_ADDRESS".into(),
                "unix:path=/nonexistent/cosmic-pass-test".into(),
            ),
        ]
    }

    /// A runner that spawns through the production code path -- process group, null stdin,
    /// timeouts, env injection -- with this isolation layered on top.
    pub fn runner(&self, cli: &RealCli) -> TokioRunner {
        TokioRunner::new(&cli.path).with_env(self.vars())
    }

    /// Runs the binary inside this home and hands back the raw result, for the few probes that
    /// need the exit status or the stderr text that `TokioRunner` deliberately hides.
    pub fn raw(&self, cli: &RealCli, args: &[&str]) -> RawOutput {
        RawOutput::capture(&cli.path, args, self.vars())
    }

    /// Everything under this home, relative to it. Used to prove a probe wrote nothing
    /// anywhere else.
    pub fn files_written(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        walk(&self.dir, &self.dir, &mut found);
        found.sort();
        found
    }
}

/// Where probe homes live: inside the build directory, never the developer's profile.
///
/// Wiped by `cargo clean` like anything else under `target/`. Losing them costs nothing -- the
/// keyring keys are addressed by path, so recreating a home finds its key again.
fn probe_home_root() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
        PathBuf::from,
    );
    target.join("pass-cli-probe-homes")
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_path_buf());
        }
    }
}

// -------------------------------------------------------------------------------------------
// Raw invocation

/// A finished run, with everything `TokioRunner` drops: the exit status and the stderr text.
///
/// `TokioRunner` hides both on purpose -- it maps a failure straight to a `PassError` and wipes
/// stdout -- which is right for the app and wrong for a probe asking "did the argument parser
/// accept this, or did authentication reject it?". Probes that ask that question use this;
/// every probe that exercises the app's own path uses the runner.
pub struct RawOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl RawOutput {
    fn capture(bin: &Path, args: &[&str], env: Vec<(String, String)>) -> Self {
        Self::try_capture(bin, args, env, PROBE_TIMEOUT).unwrap_or_else(|reason| panic!("{reason}"))
    }

    /// Runs the binary to completion, or kills it once `timeout` has passed.
    ///
    /// `std::process::Command::output` waits forever, which would let a hung `pass-cli` wedge
    /// the whole test binary rather than fail it (FR-013, research.md D10). Probes that go
    /// through `TokioRunner` already carry the app's own timeout; this is the same guarantee
    /// for the probes that need the exit status and stderr the runner hides.
    ///
    /// Returns the reason as an `Err` rather than panicking so `probe_version` can turn it
    /// into a `Resolution::Fail` instead of an unwind out of a `OnceLock` initialiser.
    pub fn try_capture(
        bin: &Path,
        args: &[&str],
        env: Vec<(String, String)>,
        timeout: Duration,
    ) -> Result<Self, String> {
        let mut child = std::process::Command::new(bin)
            .args(args)
            // The same three the production runner sets, so a raw probe and a runner probe
            // differ only in what they capture.
            .env("PASS_LOG_LEVEL", "off")
            .env("PROTON_PASS_NO_UPDATE_CHECK", "1")
            .env("PROTON_PASS_LINUX_KEYRING", "dbus")
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run {} {args:?}: {e}", bin.display()))?;

        // Both pipes are drained on their own threads. A child that fills a pipe buffer blocks
        // on the write, which would look exactly like a hang and burn the whole timeout.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());

        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(e) => return Err(format!("waiting for {} {args:?}: {e}", bin.display())),
            }
            if Instant::now() >= deadline {
                // Kill, then reap: an abandoned child would outlive the run and keep its
                // isolated home from being removed.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} {args:?} did not finish within {timeout:?} and was killed",
                    bin.display()
                ));
            }
            std::thread::sleep(POLL_INTERVAL);
        };

        Ok(Self {
            status: status.code(),
            stdout: stdout
                .join()
                .map_err(|_| "stdout reader panicked".to_owned())?,
            stderr: stderr
                .join()
                .map_err(|_| "stderr reader panicked".to_owned())?,
        })
    }

    /// How the app would read this failure.
    pub fn classified(&self) -> PassError {
        classify(&Failure {
            stderr: &self.stderr,
            ..Failure::default()
        })
    }

    /// Asserts every token is present in stdout, naming the ones that are not.
    ///
    /// Tokens, never a snapshot of the whole text: upstream rewords help prose freely, and a
    /// snapshot would fail on every reword until people learned to accept it blindly.
    pub fn assert_stdout_has(&self, clause: &str, tokens: &[&str]) {
        assert_eq!(
            self.status,
            Some(0),
            "{clause}: expected exit 0, got {:?}\nstderr: {}",
            self.status,
            self.stderr
        );
        let missing: Vec<&str> = tokens
            .iter()
            .copied()
            .filter(|t| !self.stdout.contains(t))
            .collect();
        assert!(
            missing.is_empty(),
            "{clause}: pass-cli no longer offers {missing:?}\n--- stdout ---\n{}",
            self.stdout
        );
    }
}

/// Reads one pipe to end of file on its own thread, so neither pipe can back up while the
/// other is being read or while the deadline is being polled.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

// -------------------------------------------------------------------------------------------
// Live scenarios

/// Whether a live scenario could be exercised against the account that was signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Covered,
    NotCovered { reason: String },
}

/// One read-only end-to-end exercise against a signed-in account.
#[derive(Debug, Clone)]
pub struct LiveScenario {
    pub name: &'static str,
    pub prerequisite: &'static str,
    pub outcome: Outcome,
}

impl LiveScenario {
    pub fn covered(name: &'static str, prerequisite: &'static str) -> Self {
        Self {
            name,
            prerequisite,
            outcome: Outcome::Covered,
        }
    }

    pub fn not_covered(
        name: &'static str,
        prerequisite: &'static str,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            name,
            prerequisite,
            outcome: Outcome::NotCovered {
                reason: reason.into(),
            },
        }
    }

    /// Prints the scenario's line of the coverage report.
    ///
    /// Fixed vocabulary: the scenario name, then `covered` or `not covered: <reason>`. Nothing
    /// read from the account is ever formatted in here.
    pub fn report(&self) {
        match &self.outcome {
            Outcome::Covered => eprintln!("SCENARIO {}: covered", self.name),
            Outcome::NotCovered { reason } => {
                eprintln!("SCENARIO {}: not covered: {reason}", self.name);
            }
        }
    }
}

// -------------------------------------------------------------------------------------------
// Fixture shapes

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueKind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

impl ValueKind {
    fn of(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(_) => Self::Bool,
            serde_json::Value::Number(_) => Self::Number,
            serde_json::Value::String(_) => Self::String,
            serde_json::Value::Array(_) => Self::Array,
            serde_json::Value::Object(_) => Self::Object,
        }
    }
}

/// The comparable skeleton of a JSON document: which key paths exist and what kind of value
/// sits at each. Carries no value, so a shape derived from a real account is safe to print.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixtureShape {
    pub paths: BTreeSet<(String, ValueKind)>,
}

impl FixtureShape {
    pub fn of(value: &serde_json::Value) -> Self {
        let mut paths = BTreeSet::new();
        collect(value, "", &mut paths);
        Self { paths }
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        Ok(Self::of(&serde_json::from_slice(bytes)?))
    }

    /// Key paths present here but not in `other`, and present in `other` but not here.
    pub fn diff(&self, other: &Self) -> (Vec<String>, Vec<String>) {
        let only = |a: &BTreeSet<(String, ValueKind)>, b: &BTreeSet<(String, ValueKind)>| {
            a.difference(b)
                .map(|(path, kind)| format!("{path} ({kind:?})"))
                .collect()
        };
        (
            only(&self.paths, &other.paths),
            only(&other.paths, &self.paths),
        )
    }

    /// Drops every path that does not start with one of `prefixes`.
    ///
    /// Item listings differ by which item kinds a vault happens to hold, so a whole-document
    /// comparison would report an absent kind as a removed field. Narrowing to the kinds both
    /// sides carry keeps the report about upstream's schema.
    pub fn restricted_to(&self, prefixes: &BTreeSet<String>) -> Self {
        Self {
            paths: self
                .paths
                .iter()
                .filter(|(path, _)| prefixes.iter().any(|p| path.starts_with(p.as_str())))
                .cloned()
                .collect(),
        }
    }

    /// The distinct first path segments, used to build the prefix set for `restricted_to`.
    pub fn roots(&self) -> BTreeSet<String> {
        self.paths
            .iter()
            .map(|(path, _)| path.split('.').next().unwrap_or(path).to_owned())
            .collect()
    }
}

fn collect(value: &serde_json::Value, path: &str, out: &mut BTreeSet<(String, ValueKind)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                out.insert((child_path.clone(), ValueKind::of(child)));
                collect(child, &child_path, out);
            }
        }
        serde_json::Value::Array(items) => {
            let child_path = format!("{path}[]");
            for child in items {
                out.insert((child_path.clone(), ValueKind::of(child)));
                collect(child, &child_path, out);
            }
        }
        _ => {}
    }
}

// -------------------------------------------------------------------------------------------
// Shared argv and timeouts

/// Generous enough that a slow machine does not flake, short enough that a hung binary fails
/// the suite instead of wedging it.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Listing a real vault is the slowest thing either suite does.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// How often `try_capture` checks whether the child has exited. Small enough to add nothing
/// measurable to a probe that returns in about 10 ms.
const POLL_INTERVAL: Duration = Duration::from_millis(2);

pub fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_owned()).collect()
}

/// Counts how often each `PassError` variant name appears, for reports that must not print the
/// errors themselves.
pub fn tally(errors: &[PassError]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for error in errors {
        *counts
            .entry(
                format!("{error:?}")
                    .split(' ')
                    .next()
                    .unwrap_or("?")
                    .to_owned(),
            )
            .or_insert(0) += 1;
    }
    counts
}

/// `OsString` helper for building an argv from owned share/item ids.
pub fn os(value: &str) -> OsString {
    OsString::from(value)
}
