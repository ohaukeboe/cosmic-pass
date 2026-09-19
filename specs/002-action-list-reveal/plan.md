# Implementation Plan: One surface for item fields

**Branch**: `002-action-list-reveal` | **Date**: 2026-09-19 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-action-list-reveal/spec.md`

## Summary

Collapse the two per-item screens into one. The action list (Tab) becomes the only place that
lists an item's fields: `Ctrl+R` reveals the highlighted row in place, one row at a time, and a
one-time-code row reveals as a live code with its countdown. The detail pane (`Ctrl+I`), its
`Mode::Detail` state, its `OpenDetail` action and its view module are deleted. User-defined
text fields — the ones `pass-cli` never gives the app a value for — are dropped at the parser
so they never reach the cache, the list, or a copy shortcut.

Technically this is three independent slices over existing machinery:

1. **Reveal moves to the action list.** `Msg::ToggleReveal` stops cycling over secret fields
   and instead targets the highlighted action row. `ActionEntry` gains the row's position in
   `item.fields` so the reveal target can be pinned exactly as it is today; TOTP rows reveal
   through the existing `view.totp` + `Msg::Tick` refresh path.
2. **Detail pane deleted.** `Mode::Detail`, `Msg::OpenDetail`, `Action::OpenDetail`,
   `src/app/view/detail.rs` and the `in_detail` guards go; every guard becomes
   "is the action list open for this key". Preferences must survive a stored `open_detail`
   binding, so the shortcut map gets a tolerant deserializer.
3. **Dead fields dropped.** `parse::custom_fields` stops pushing `FieldRef::unstored` for
   `CustomFieldKind::Text | Other`; the cache format version is bumped so an existing cache
   containing those rows is discarded rather than rendered.

## Technical Context

**Language/Version**: Rust, edition 2024, rust-version 1.93

**Primary Dependencies**: libcosmic (iced, wayland layer-shell), `cosmic-config` for
preferences, `secrecy` for in-memory secrets, `tokio` + `tokio-util` for fetches and
cancellation, `postcard` + `chacha20poly1305` for the cache, `serde`/`serde_json` for
(de)serialization, `insta` for snapshot tests

**Storage**: `cosmic-config` (RON, one value per key) for preferences; encrypted `postcard`
metadata cache at `$XDG_CACHE_HOME/cosmic-pass/cache.bin`. No schema change beyond a
`CACHE_FORMAT_VERSION` bump.

**Testing**: `cargo nextest` (unit tests in-module, acceptance tests in `tests/story*.rs`),
`cargo llvm-cov` with an 80% floor, `insta` snapshots for parser output

**Target Platform**: Linux / COSMIC desktop (Wayland layer-shell popup)

**Project Type**: Single Rust crate — desktop app with a pure `src/core` reducer and thin
libcosmic adapter in `src/app`

**Performance Goals**: Unchanged. Reveal must not add a fetch on opening the action list;
values are fetched only on the reveal press. The 1 Hz `Msg::Tick` already runs; it must not
fetch a one-time code unless one is revealed.

**Constraints**: `src/core` must not import libcosmic or perform IO. Secrets never logged,
`Debug`-printed, serialized, or passed in argv. Revealed values live only in `view.revealed` /
`view.totp` and are dropped on mask, highlight move, leaving the list, and window close.

**Scale/Scope**: ~8 source files touched, one deleted (`src/app/view/detail.rs`), one test
file rewritten (`tests/story3_detail_pane.rs`), three contract docs amended.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|-----------|------|--------|
| I. Code quality | `just fmt` + `just lint` (clippy `-D warnings`) clean; no dead code left behind by the removal; no suppressions added | PASS — the removal is subtractive; every `Mode::Detail` arm, the `detail.rs` module and the `Pane::Detail` variant go with it |
| II. Test-first | Every behavior change starts with a failing test: reveal-follows-highlight, TOTP reveal, removed shortcut, tolerant preference load, dropped text fields | PASS — task order in `tasks.md` will put each test before its change |
| III. Layered coverage | Reducer logic unit-tested in `src/core/state.rs`; parser change snapshot-tested; each user story has an acceptance test under `tests/` | PASS — `tests/story3_detail_pane.rs` is rewritten into `tests/story3_reveal_in_action_list.rs` for the new surface, not deleted |
| IV. Simplicity | No new dependency, no new layer. One new field on `ActionEntry`, one tolerant deserializer, one deletion | PASS — see Complexity Tracking for the deserializer, which is the only addition |
| V. Contracts & docs | `contracts/keyboard.md` and `contracts/config.md` of feature 001 are amended, `cache-format.md` notes the version bump, `README.md` keys table stays accurate | PASS — doc tasks ship in the same change |

Post-design re-check: **PASS**. Phase 1 added no dependency, no new module, and no new
persisted state; `Mode` loses a variant and `ViewState` loses none.

## Project Structure

### Documentation (this feature)

```text
specs/002-action-list-reveal/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output (deltas to feature 001 contracts)
│   ├── keyboard.md
│   └── config.md
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks — not created here)
```

### Source Code (repository root)

```text
src/
├── config.rs                 # Action::OpenDetail removed; tolerant shortcut map load
├── model.rs                  # CACHE_FORMAT_VERSION bump
├── core/
│   ├── actions.rs            # ActionEntry gains the row's field position
│   ├── effects.rs            # FetchReveal/FetchTotp doc comments follow the new surface
│   └── state.rs              # Mode::Detail, Msg::OpenDetail removed; reveal follows the highlight
├── pass/
│   └── parse.rs              # custom text fields dropped
└── app/
    ├── keys.rs               # Ctrl+R maps in action-list mode; Ctrl+I unmapped
    └── view/
        ├── mod.rs            # Pane::Detail removed
        ├── actions.rs        # revealed value, countdown, reveal hint, empty-list message
        ├── detail.rs         # DELETED
        └── preferences.rs    # unchanged (driven by Action::ALL)

tests/
├── story2_other_fields.rs            # dropped text fields must not appear as copy targets
└── story3_reveal_in_action_list.rs   # renamed from story3_detail_pane.rs
```

**Structure Decision**: Unchanged single-crate layout. The feature is a subtraction plus a
relocation inside the existing `core` (pure reducer) / `app` (libcosmic adapter) split; no new
module or boundary is introduced, and `src/core` stays IO-free.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|--------------------------------------|
| Tolerant deserializer for `Preferences::shortcuts` (a `Shortcuts` newtype reading string keys and skipping unknown ones) | FR-110: an existing config binds `open_detail`; with the plain `BTreeMap<Action, KeyChord>` the whole `shortcuts` value fails to deserialize and every user-set chord silently reverts to default | Keeping a hidden `Action::OpenDetail` variant out of `Action::ALL` would also parse, but leaves a dead variant the constitution forbids and repeats the problem for every future action rename |
