# Research: Windowless `serve` mode

Each decision names what was chosen, why, and what was rejected. Code references are to the
tree at `f4c54a6`.

## R1. Where the code lives

- **Decision**: A new module `src/serve/` with two files. `src/serve/protocol.rs` holds the
  pure parts: `Intent` (serde, tagged by `t`), `impl From<Intent> for Msg`, `Snapshot` and
  `Snapshot::of(&Model, now)`. `src/serve/mod.rs` holds the IO loop and `pub fn main`.
- **Rationale**: The projection and the intent mapping are pure and carry most of the logic;
  keeping them apart from stdin/stdout lets them be unit-tested without a runtime, as `src/core`
  is. `src/core` itself is not the place: the snapshot is a wire format for one adapter, the
  same layer as `src/app/view`, and `src/core` must not know about adapters.
- **Alternatives**: one `src/serve.rs` (the dot-emacs task text) mixes the pure projection with
  the loop, so the projection's tests need the loop; `src/core/snapshot.rs` puts an adapter's
  wire format in the domain layer.

## R2. Driving the reducer without iced

- **Decision**: The loop owns the `Model` and a `tokio::sync::mpsc` channel of `Msg`. Each
  `Effect` goes through `runtime::execute`; `Step::Future` and `Step::Stream` are spawned and
  send their messages into the channel; `ShowWindow`, `HideWindow` and `Unhandled` are dropped
  (`visible` in the snapshot is read from `model.view.visible`). One `tokio::select!` waits on
  the next stdin line, the next channel message, and a 1 s interval.
- **Rationale**: `runtime::execute` is already the shared effect runner (`src/runtime.rs:1`,
  "Shared by the app and the test harness"); `testing::Harness::send_no_wait` is the same shape
  with a `JoinSet`. Reusing it means `serve` copies, reveals, caches and signs in exactly as the
  popup does (spec FR-013, FR-015).
- **Alternatives**: a second effect runner (duplicates `execute`); running the iced
  application headless (iced has no windowless mode for layer-shell apps and would still claim
  the single-instance name, spec FR-001).

## R3. Testable seam for the loop

- **Decision**: `pub async fn run(input: impl AsyncBufRead + Unpin, output: impl AsyncWrite +
  Unpin, deps: Deps, prefs: Preferences, clock: impl Fn() -> i64) -> io::Result<()>`.
  `main` builds the real `Deps`, wraps `tokio::io::stdin()`/`stdout()` and maps the result to an
  exit code. Tests call `run` with `tokio::io::duplex` pipes, `FakeBackend`, `FakeClipboard` and
  a fixed clock.
- **Rationale**: Satisfies "driven with the fake backend through the real `serve` loop" (contract
  Tests) without spawning a process per case, and lets one-time-code snapshots be asserted with a
  known `remaining`. Only argument parsing and exit codes need the real binary.
- **Alternatives**: process-only tests through `CARGO_BIN_EXE_cosmic-pass` and
  `tests/fixtures/fake-pass-cli` (slow, and the fake CLI cannot script every failure state the
  `FakeBackend` can).

## R4. Reading stdin

- **Decision**: Enable tokio's `io-std` feature and read with
  `BufReader::new(tokio::io::stdin()).lines()`.
- **Rationale**: tokio is already a dependency; `io-std` is one of its own features, not a new
  crate. `lines()` gives the end-of-file signal FR-016 needs.
- **Alternatives**: a `std::thread` reading blocking stdin into the channel (same effect, more
  code; tokio's stdin is that thread already).

## R5. Ticks for one-time codes

- **Decision**: The 1 s interval in R2 runs always; on each tick the loop sends
  `Msg::Tick(clock())` only when `model.revealed_totp().is_some()` and `model.view.visible`.
- **Rationale**: Same condition as the app's subscription (`src/app/mod.rs:490`). The snapshot
  dedupe (R6) suppresses output when nothing changed, and `remaining` changes each second, so a
  shown code yields one snapshot per second (FR-012). An always-on interval is one timer wake per
  second; not worth a start/stop state machine.
- **Alternatives**: starting and stopping the interval with the reveal.

## R6. When to write a snapshot

- **Decision**: After every reducer update, serialise `Snapshot::of(&model, clock())` to one
  line; write it only if it differs from the last line written. Keep the last line as a
  `secrecy::SecretString` so the copy held for comparison is zeroed when replaced.
- **Rationale**: Contract: "one snapshot after every reducer update that changes what is
  below". Comparing the serialised line is exact and needs no `PartialEq` on every nested type.
- **Alternatives**: a dirty flag inside the reducer (touches every message arm).

## R7. Serialising secrets (deviation from a project rule)

- **Decision**: `Snapshot` puts the one revealed plaintext and the shown one-time code into the
  JSON line it writes to stdout. It does not derive `Debug`; the field holding plaintext is
  exposed from the `SecretString` only inside `Snapshot::of`; the line is written to stdout and
  nowhere else; stderr logging never formats a snapshot or an intent's `text`.
- **Rationale**: `CLAUDE.md` says never serialise a secret. Revealing a value to a front end that
  draws it is the feature (spec US2), and the popup does the same by drawing it. The pipe to the
  front end is the drawing surface here. Contract guarantees 1–4 bound the exposure exactly as
  the popup's FR-102 does. Recorded in the plan's Complexity Tracking.
- **Alternatives**: sending a handle and a second "fetch value" call (the plaintext still crosses
  the pipe, later, and the front end must hold it); not supporting reveal (drops US2 scenario 2).

## R8. Sign-in progress lines

- **Decision**: Add `login_lines: Vec<String>` to `Model`: `StartLogin` clears it,
  `LoginLine(line)` pushes, `LoginFinished` clears it. The snapshot's `login` is
  `{url: model.login_url, lines: model.login_lines}` while `session` is `LoggingIn`, else `null`.
- **Rationale**: The contract (and dot-emacs FR-020) show progress lines; the reducer today keeps
  only the URL (`src/core/state.rs:577`). A field in the model is the smallest change and stays
  pure. `pass-cli login` prints status and the URL, not secrets.
- **Alternatives**: drop `lines` from the contract (loses the front end's progress display); keep
  lines in the serve loop (state outside the reducer, untestable through `Model`).

## R9. `locked` and the keyring

- **Decision**: `session: "locked"` means the `pass-cli` session is locked
  (`PassError::Locked`, classified from "Session is locked", `src/pass/error.rs:77`). The
  authoritative contract is corrected; the dot-emacs copy and its T023 status text must follow.
- **Rationale**: Nothing else sets `SessionState::Locked`. An unreadable keyring only disables the
  cache (`src/cache/keystore.rs:20`) and is not surfaced in the model.
- **Alternatives**: add a keyring state to the model (new behaviour nobody specified here).

## R10. Item kinds on the wire

- **Decision**: `Login`→`login`, `CreditCard`→`card`, `Note`→`note`, `Identity`→`identity`,
  `Alias`→`alias`, `Custom`→`custom`; `SshKey`, `Wifi`, `Unknown(_)`→`other`.
- **Rationale**: The contract's seven kinds. Adding `ssh_key` and `wifi` later is a new value of
  an existing field; front ends must already handle `other`.

## R11. Field entries and the busy flags

- **Decision**: `fields` is `model.actions()` in order. Per entry: `label`, `secret =
  source.is_secret()`; `value` is the stored `FieldRef.value` for a non-secret field,
  `revealed_value(field_index)` for a secret one, else `null`; `revealing` is
  `revealing_field(field_index)` for a field or `revealing_totp()` for a code; `totp` is
  `{code, remaining(now)}` when `revealed_totp()` names this entry's field. `busy.refreshing` is
  `data.refreshing`, `busy.fetching` is `view.pending.is_some()`, `busy.copying` is
  `view.copying`.
- **Rationale**: The same reads the popup's action list makes (`src/app/view/actions.rs:52-100`).
  This is the second consumer of that logic; the constitution allows duplication until the third,
  so it is not extracted now.

## R12. Exit codes

- **Decision**: `0` on stdin EOF; `2` from clap for a missing or out-of-range
  `--clipboard-clear-secs` (`value_parser!(u32).range(10..=600)`, matching
  `config::CLIPBOARD_CLEAR_RANGE`); `1` when writing stdout fails (FR-016), logged once.
- **Rationale**: clap already exits 2 on a usage error before `main` writes anything. A broken
  stdout is a failure, not a clean stop; the front end restarts either way.

## R13. Performance target (SC-001)

- **Decision**: An `#[ignore]` test in `tests/serve_protocol.rs` that loads 2,000 fake items,
  sends one `query`, and asserts its snapshot arrives within 100 ms; named
  `search_bench_serve_query` so the existing `just bench` filter (`search_bench`, release) picks it
  up with no recipe change.
- **Rationale**: Timing assertions in the debug, parallel `just test` run are flaky (constitution
  III, determinism). The existing bench pattern is the project's answer.
