# Implementation Plan: Windowless `serve` mode

**Branch**: `004-serve-mode` | **Date**: 2026-11-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/004-serve-mode/spec.md`

## Summary

Add `cosmic-pass serve --clipboard-clear-secs <10..600>`: a windowless process that feeds the
existing reducer (`core::state::Model`) with intents read as JSON lines from stdin, runs its
effects through the shared `runtime::execute`, and writes a complete JSON snapshot of the visible
state to stdout after every update that changes it. The pure parts (intent decoding, the
`Model` → `Snapshot` projection) sit in `src/serve/protocol.rs`; the IO loop in
`src/serve/mod.rs` is generic over its reader, writer and clock so tests drive it in-process with
`FakeBackend`. The only reducer change is keeping sign-in progress lines. Wire format:
[`contracts/serve-protocol.md`](contracts/serve-protocol.md).

## Technical Context

**Language/Version**: Rust, edition 2024 (toolchain from the flake)

**Primary Dependencies**: existing only — tokio (add its own `io-std` feature), serde,
serde_json, clap, secrecy, tracing; no new crates

**Storage**: existing encrypted metadata cache and usage table, read and written as the popup
does; no preferences written

**Testing**: cargo nextest; `insta` (json) for snapshot shapes; in-process loop tests over
`tokio::io::duplex`; process tests via `CARGO_BIN_EXE_cosmic-pass` for exit codes

**Target Platform**: Linux, Wayland session (clipboard helper uses wlr data-control); started as a
child of a Quickshell front end

**Project Type**: single crate, desktop app with CLI subcommands

**Performance Goals**: with 2,000 items, a `query` intent's snapshot within 100 ms (SC-001)

**Constraints**: no window, no single-instance name, no cosmic-config; nothing secret on stderr,
in logs or files; one line per message; at most 50 rows

**Scale/Scope**: one front end per process; up to a few thousand items

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle                | Status    | How                                     |
|--------------------------|-----------|-----------------------------------------|
| I. Code quality          | Pass      | clippy -D warnings; errors propagated   |
| II. Test-first           | Pass      | protocol and loop tests before code     |
| III. Layered coverage    | Pass      | unit, in-process loop, process          |
| III. Story acceptance    | Pass      | a test group per user story             |
| III. 80% changed code    | Pass      | only `serve::main` is untested glue     |
| IV. Simplicity           | Pass      | reuses `execute`, `Model`; no new crate |
| V. Contracts and docs    | Pass      | contract file; README section           |
| Perf target has a test   | Pass      | `search_bench_serve_query` (R13)        |
| `src/core` stays pure    | Pass      | only `login_lines` added; no IO         |
| Never serialise a secret | Deviation | see Complexity Tracking                 |

**Post-design re-check**: unchanged. Design adds two source files, one test file, one `Model`
field, one tokio feature; the single deviation is justified below.

## Project Structure

### Documentation (this feature)

```text
specs/004-serve-mode/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R13
├── data-model.md        # Model → Snapshot projection, intent table
├── quickstart.md        # validation steps
├── contracts/
│   └── serve-protocol.md  # authoritative wire contract
├── checklists/
│   └── requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/
├── cli.rs               # + Command::Serve { clipboard_clear_secs }, clap range 10..=600
├── main.rs              # + dispatch Command::Serve to serve::main before app::run
├── lib.rs               # + pub mod serve
├── core/state.rs        # + Model::login_lines (StartLogin/LoginLine/LoginFinished)
└── serve/
    ├── mod.rs           # pub fn main(secs) -> ExitCode; pub async fn run(input, output, deps, prefs, clock)
    └── protocol.rs      # Intent (+ From<Intent> for Msg), Snapshot, Snapshot::of(&Model, now)

tests/
└── serve_protocol.rs    # loop tests per user story, guarantees 1–3, garbage line, EOF,
                         # exit 2 on bad flag, search_bench_serve_query (#[ignore])

Cargo.toml               # tokio features + "io-std"
README.md                # + "Serve mode" section
```

**Structure Decision**: Single crate, as today. `src/serve/` is a peer of `src/app/`: both are
thin adapters over `src/core` and `src/runtime.rs`. `serve::main` builds `Deps` the way
`app::CosmicPass::init` does (`PassCli` over `TokioRunner::from_env`, `HelperClipboard::from_env`
falling back to `NoClipboard`, `CacheStore::from_env(Oo7KeyStore)`) with
`save_preferences: false`, and `Preferences::default()` with `clipboard_clear_secs` from the flag
and `max_results = 50`.

## Implementation notes

- **Loop** (research R2, R3, R5, R6): `Msg::Startup` first, then write the first snapshot; then
  `select!` over stdin lines, effect messages from an `mpsc` channel, and a 1 s interval. After
  each `Model::update`, run every effect: `Step::Future` → spawn, send its `Some(msg)`;
  `Step::Stream` → spawn, forward each msg; `ShowWindow`/`HideWindow`/`Unhandled` → drop. Then
  project, serialise, compare to the last line, write + flush if different. `ExpireNotice` goes
  through `execute` like every other effect, so notices clear on time.
- **EOF**: stdin `None` returns `Ok(())`; pending effect tasks are dropped with the runtime. A
  copy in flight is owned by the clipboard helper process, which outlives `serve` and still
  clears on time.
- **Logging**: parse failures log `"skipping unparseable intent"` with the serde error's
  line/column only, never the line (a `query` carries what the user typed). Snapshots are never
  logged.
- **Errors**: a stdout write error returns `Err`; `main` logs it once and exits 1.

## Complexity Tracking

1. **A revealed secret is serialised** (against `CLAUDE.md`). Needed: reveal (US2) means the
   front end must draw the plaintext. Rejected: a handle plus a later fetch still sends it; no
   reveal drops US2.
2. **`Model::login_lines` added to the reducer.** Needed: the front end shows sign-in progress
   (contract `login.lines`). Rejected: lines kept in the loop are state outside the reducer.
3. **Field-entry logic mirrors `app/view/actions.rs`.** Needed: second consumer of the same
   reads (research R11). Rejected: extracting at two uses goes against the rule of three.

Mitigations for item 1 (research R7): `Snapshot` has no `Debug`; plaintext is exposed from
`SecretString` only inside `Snapshot::of`; the line goes to stdout only; the comparison copy is a
`SecretString`; tests assert guarantees 1–3 over every snapshot they read.
