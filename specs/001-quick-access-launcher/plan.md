# Implementation Plan: Proton Pass Quick Access

**Branch**: `001-quick-access-launcher` | **Date**: 2026-09-17 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-quick-access-launcher/spec.md`

## Summary

A keyboard-driven popup for COSMIC that searches Proton Pass items and copies passwords,
usernames, one-time codes, and other fields to the clipboard. It works like 1Password Quick
Access.

Technical approach:

- **Toolkit and window**: a resident libcosmic (iced) app, modelled on `cosmic-launcher`. A
  single-instance D-Bus call toggles a layer-shell popup.
- **Proton Pass access**: all calls go through `pass-cli` subprocesses behind a trait
  boundary.
- **Search**: runs locally on non-secret metadata with `nucleo-matcher`.
- **Cache**: the metadata is kept in memory and in an XChaCha20-Poly1305 file whose key lives
  in the Secret Service.
- **Secrets**: fetched only on demand and handed to a clipboard helper subprocess. The helper
  offers the value with the password-manager hint and clears it on timeout, but only if it
  still owns the selection.

## Technical Context

**Language/Version**: Rust stable, edition 2024 (toolchain from `shell.nix`; MSRV pinned to
libcosmic's requirement)

**Primary Dependencies**:

- UI: `libcosmic` (git, pinned rev; features `winit`, `wayland`, `tokio`,
  `single-instance`, `dbus-config`, `autosize`, `multi-window`)
- Runtime: `tokio`, `tokio-util`
- Serialization and CLI: `serde`, `serde_json`, `clap`
- Search: `nucleo-matcher`
- Clipboard: `wl-clipboard-rs`
- Cache: `oo7` (Secret Service), `chacha20poly1305`, `postcard`, `tempfile`, `dirs`
- Secret handling: `secrecy`, `zeroize`
- Errors and logging: `thiserror`, `tracing`

**Storage**:

- Encrypted metadata cache at `$XDG_CACHE_HOME/cosmic-pass/cache.bin`
  ([contracts/cache-format.md](./contracts/cache-format.md))
- Preferences in `cosmic-config` ([contracts/config.md](./contracts/config.md))
- No secrets on disk.

**Testing**:

- Test runner and coverage: `cargo nextest`, `cargo llvm-cov`
- Snapshot tests: `insta`
- Integration tests: a fake `pass-cli` shell script
- UI tests: `iced_test` (pop-os fork, same rev), if compatible (research V2)

**Target Platform**: Linux, COSMIC desktop (cosmic-comp) on Wayland; non-sandboxed install
(data-control protocol required)

**Project Type**: Desktop application (single binary, resident background process)

**Performance Goals**:

- Window visible and typing accepted ≤ 100 ms after shortcut (p95)
- Results ≤ 50 ms per keystroke at 5,000 items
- Shortcut to copied password < 3 s

**Constraints**:

- UI thread never blocks on `pass-cli`, keyring, or disk
- Zero dropped keystrokes during refresh
- Secrets never written to disk or logs, and never kept after hand-off
- Clipboard cleared at timeout ± 1 s only if still owned
- At most 4 concurrent `pass-cli` processes

**Scale/Scope**: One user, one Proton account, up to ~5,000 items across any number of
vaults; 4 UI modes (list, actions, detail, preferences); read-only

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Pre-research | Post-design |
|-----------|------|--------------|-------------|
| I. Code Quality | `rustfmt` + `clippy -D warnings` clean; typed errors (`thiserror`), no swallowed errors; no secrets in source | PASS (planned in tooling) | PASS: `just check` defined; error enums in [contracts/pass-cli.md](./contracts/pass-cli.md) |
| II. Test-First | Failing test before each behavior; regression test per bug | PASS (tasks will be ordered test-first) | PASS: trait boundaries (`CommandRunner`, `PassBackend`, `Clipboard`, `KeyStore`) make every behavior testable before implementation |
| III. Layered Coverage | Unit + integration + one acceptance test per story; ≥ 80% changed-line coverage; deterministic | PASS with note | PASS with deviation (see Complexity Tracking #1): story acceptance tests run the full app controller against the fake `pass-cli`, fake keyring and fake clipboard; compositor behavior (layer-shell focus, real clipboard) is covered by quickstart V2–V4 |
| IV. Simplicity | Single crate; justified complexity only | PASS | PASS with justified items #2–#3 |
| V. Contracts & Docs | Public interfaces documented; README + build/test docs updated | PASS | PASS: `contracts/` covers CLI, D-Bus, keyboard, config, cache format, consumed `pass-cli`; README and `CLAUDE.md`/`AGENTS.md` "Build & Test" update is a task |
| Quality Standards | Single commands; reproducible env | PASS | PASS: `justfile` + `shell.nix` (research R10). Resolves constitution `TODO(TECH_STACK)` |
| Workflow | `bd` tracking; conventional commits; no agent commits without approval | PASS | PASS |

No unjustified violations. The gate passes.

## Project Structure

### Documentation (this feature)

```text
specs/001-quick-access-launcher/
├── plan.md              # This file
├── research.md          # Phase 0
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1
├── contracts/           # Phase 1
│   ├── cli.md
│   ├── keyboard.md
│   ├── config.md
│   ├── cache-format.md
│   └── pass-cli.md
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
Cargo.toml
Cargo.lock
justfile                   # fmt, lint, test, cov, check, bench, run, install-user,
                           # capture-fixtures, leak-scan
shell.nix                  # rust toolchain + wayland/graphics libs + nextest/llvm-cov/just
rustfmt.toml
clippy.toml

src/
├── main.rs                # clap parsing; dispatch to app or clipboard-serve
├── lib.rs                 # module wiring; used by integration tests
├── app/                   # libcosmic glue only (thin)
│   ├── mod.rs             # cosmic::Application impl: update/view/subscription/dbus_activation
│   ├── surface.rs         # layer-shell show/hide, focus-loss, debounce
│   ├── view/              # list.rs, actions.rs, detail.rs, preferences.rs, status.rs
│   └── keys.rs            # key event → core::Msg mapping using configured chords
├── core/                  # pure logic, no UI or IO types
│   ├── state.rs           # ViewState, DataState, SessionState, reducer (Msg → Effects)
│   ├── effects.rs         # Effect enum executed by app layer (fetch, copy, persist…)
│   ├── search.rs          # nucleo haystack + ranking + recent-first ordering
│   ├── actions.rs         # CopyAction derivation per ItemKind
│   ├── totp.rs            # countdown math
│   └── usage.rs           # UsageRecord ranking/eviction
├── pass/                  # pass-cli boundary
│   ├── runner.rs          # CommandRunner trait + tokio impl (timeouts, pgroup, semaphore)
│   ├── backend.rs         # PassBackend trait + PassCli impl
│   ├── parse.rs           # lenient JSON → Vault/ItemSummary; secret stripping
│   └── error.rs           # PassError + stderr classification
├── cache/
│   ├── keystore.rs        # KeyStore trait + oo7 impl
│   ├── crypto.rs          # envelope encrypt/decrypt
│   └── store.rs           # load/save/delete, atomic write, debounce
├── clipboard/
│   ├── mod.rs             # Clipboard trait + ClipboardJob (helper process, timer)
│   └── serve.rs           # `clipboard-serve` subcommand (wl-clipboard-rs copy_multi)
├── model.rs               # ShareId, ItemId, ItemKey, Vault, ItemSummary, FieldRef, ItemKind
└── config.rs              # Preferences (cosmic-config), validation, defaults

tests/
├── fixtures/
│   ├── fake-pass-cli      # POSIX sh; behavior via FAKE_* env vars
│   └── pass-cli/          # redacted real JSON captures (V1)
├── pass_cli_integration.rs    # PassCli against fake binary: parsing, errors, timeouts
├── cache_integration.rs       # encrypt/persist/reload, account switch, no-secret-bytes
├── clipboard_integration.rs   # helper lifecycle with a fake serve binary (no compositor)
├── story1_copy_password.rs    # acceptance: controller + fakes
├── story2_other_fields.rs
├── story3_detail_pane.rs
├── story4_status.rs
└── search_bench.rs            # 5,000-item timing (SC-002)

data/
├── io.github.ohaukeboe.CosmicPass.desktop
├── io.github.ohaukeboe.CosmicPass.metainfo.xml
├── cosmic-pass.service
└── icons/io.github.ohaukeboe.CosmicPass.svg
```

**Structure Decision**: One Rust crate with a library target (for tests) and one binary. The
`core` module is pure and holds all behavior. `app` is a thin libcosmic adapter, and
`pass`, `cache` and `clipboard` are IO boundaries behind traits. No workspace: one binary
does not need several crates (Principle IV).

## Complexity Tracking

| # | Violation / added complexity | Why needed | Simpler alternative rejected because |
|---|------------------------------|------------|--------------------------------------|
| 1 | Story acceptance tests stop at the app controller, not the real compositor (Principle III "end to end") | Layer-shell focus and Wayland data-control need a live COSMIC session; no headless compositor harness exists for cosmic-comp | Full compositor-driven E2E would need a nested cosmic-comp in CI, which is unsupported. Covered by scripted manual checks in quickstart V2–V4. For the same reason `src/main.rs`, `src/app/mod.rs`, `src/app/surface.rs`, and `src/app/view/` are excluded from the coverage gate; keep logic out of them |
| 2 | Clipboard helper subprocess (`clipboard-serve`) | Clipboard must outlive the hidden window, carry the password-manager hint, and be cleared only while still owned (FR-015/016) | Toolkit clipboard dies with window focus; `arboard` `.wait()` cannot be cancelled; a hand-written Wayland client is more code |
| 3 | Encrypted on-disk cache + keyring dependency | User chose instant results after login/reboot without exposing account metadata (FR-024) | In-memory only was offered and declined; plaintext cache leaks which sites the user has accounts at |
