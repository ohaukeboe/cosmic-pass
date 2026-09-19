---

description: "Task list for feature 002: One surface for item fields"
---

# Tasks: One surface for item fields

**Input**: Design documents from `/specs/002-action-list-reveal/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/)

**Tests**: Test tasks are REQUIRED here. The project constitution makes test-first
(Red → Green → Refactor) non-negotiable, so every behavior task is preceded by the test that
must fail first.

**Organization**: Grouped by user story. US1 lands the replacement before US2 removes what it
replaces, so the repo is never missing a way to reveal a secret.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: US1, US2, US3 — maps to the user stories in spec.md

## Path Conventions

Single Rust crate: `src/` and `tests/` at repository root. `src/core` is pure (no libcosmic,
no IO); `src/app` is the libcosmic adapter.

---

## Phase 1: Setup

**Purpose**: Track the work and confirm a green baseline before touching anything.

- [X] T001 Create the bd epic and one issue per user story with `bd create --type=feature --title="002: one surface for item fields"` and `bd create --parent=<epic>` for US1/US2/US3, then `bd update <us1> --claim`
- [X] T002 Record a green baseline by running `just check` and noting the current coverage percentage in the bd epic's notes

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Rename the detail-era helpers to the surface that survives, so US1 and US2 both
build on the same names. Pure renames and one new predicate — no behavior change.

**⚠️ CRITICAL**: No user story work starts until this phase is complete.

- [X] T003 Rename `ViewState::detail_cancel` to `reveal_totp_cancel` and `Model::clear_detail_secrets` to `clear_revealed` in `src/core/state.rs`, updating `detail_token()` and every call site; no behavior change, existing tests stay green
- [X] T004 Add `Model::in_actions(&self, key: &ItemKey) -> bool` next to the existing `in_detail` in `src/core/state.rs`, returning true when `view.mode` is `Mode::Actions` for that key

**Checkpoint**: `just test` still green; the reveal machinery is named for the field list.

---

## Phase 3: User Story 1 - Reveal a secret from the action list (Priority: P1) 🎯 MVP

**Goal**: `Ctrl+R` in the field list reveals the highlighted row in place — plaintext for a
secret, live code plus countdown for a one-time code — one row at a time, dropped on mask,
highlight move, leave, or close. Covers FR-101 … FR-107.

**Independent Test**: Open an item's field list, highlight the password row, press `Ctrl+R`:
the value appears on that row only. Press again: masked. Reveal, then press `Down`: masked.
The detail pane still exists at this point and is untouched.

### Tests for User Story 1 ⚠️ write first, watch them fail

- [X] T005 [P] [US1] Unit test in `src/core/actions.rs` asserting `all_actions` sets `field_index` to each row's position in `item.fields`, `None` for one-time-code rows, and the primary row's index equal to the field it repeats — include an item with two fields sharing a name
- [X] T006 [P] [US1] Unit tests in `src/core/state.rs` for reveal-follows-highlight in `Mode::Actions`: pressing reveal on a secret row emits `Effect::FetchReveal` for that row's field index; pressing again masks and cancels; `revealed_value(index)` answers only for the revealed index (FR-101, FR-102)
- [X] T007 [P] [US1] Unit test in `src/core/state.rs` that `Msg::SelectNext`/`SelectPrev`/`PageDown`/`PageUp` in `Mode::Actions` clear `revealed`, `revealed_field` and `totp` and cancel the fetch in flight (FR-102, FR-103)
- [X] T008 [P] [US1] Unit test in `src/core/state.rs` that reveal on a non-secret row is a no-op — no effect, no notice, nothing revealed (FR-105)
- [X] T009 [P] [US1] Unit tests in `src/core/state.rs` for revealing a one-time-code row: emits `Effect::FetchTotp`, stores the code in `view.totp`, `Msg::Tick` past `valid_until` refetches while revealed, and `Msg::Tick` does nothing once masked (FR-104)
- [X] T010 [P] [US1] Unit tests in `src/core/state.rs` that a `Msg::RevealFetched` whose generation or index no longer matches is dropped, and that a refresh which moves, renames or removes the pinned field drops the revealed value (FR-106)
- [X] T011 [P] [US1] Unit test in `src/core/state.rs` that a failed `Msg::RevealFetched` leaves the row masked and emits a notice, and that `PassError::Cancelled` emits neither
- [X] T012 [P] [US1] Unit test in `src/app/keys.rs` that the `reveal` chord maps to `Msg::ToggleReveal` in `Mode::Actions` and to `None` in `Mode::List`
- [X] T013 [P] [US1] Unit tests in `src/app/view/actions.rs` that the value line shows the plaintext for the revealed row, the mask for other secret rows, `code · {n}s` for a revealed one-time-code row, and that the caption names the reveal chord for the highlighted secret row (FR-104, FR-107)
- [X] T014 [US1] Acceptance test `tests/story3_reveal_in_action_list.rs` covering Story 1 end to end through `Harness`: open the field list, reveal, re-reveal, move, leave, and a failing fetch — port the surviving assertions from `tests/story3_detail_pane.rs` (R8)

### Implementation for User Story 1

- [X] T015 [US1] Add `field_index: Option<usize>` to `ActionEntry` in `src/core/actions.rs` and set it in `all_actions` — the field's position in `item.fields` for field rows (including the repeated primary row), `None` for `CopySource::Totp` rows (data-model.md)
- [X] T016 [US1] Rewrite `Model::toggle_reveal` in `src/core/state.rs` to target the highlighted row: read `Mode::Actions { key, selected }`, look the row up in `self.actions()`, clear both `revealed`/`revealed_field` and `totp` first, then re-arm — `Effect::FetchReveal` pinned by `reveal_target` for a secret `field_index`, `Effect::FetchTotp` for a `Totp` row, nothing for a non-secret row. Keep the existing `Mode::Detail` cycle arm working until US2 deletes it
- [X] T017 [US1] Accept the action list in the reveal guards in `src/core/state.rs`: `Msg::RevealFetched` and `Msg::TotpFetched` pass when `in_actions(&key)` or `in_detail(&key)`, and `fetch_totp` reads the key from either mode
- [X] T018 [US1] Gate the `Msg::Tick` one-time-code refresh in `src/core/state.rs` on a revealed code (`view.totp.is_some()`), so a closed or masked row costs no `pass-cli` call (FR-103)
- [X] T019 [US1] Clear reveals on highlight movement in `src/core/state.rs`: `move_selection` in `Mode::Actions` calls `clear_revealed()` before updating `selected`
- [X] T020 [US1] Map the `reveal` chord in `Mode::Actions` in `src/app/keys.rs` (`Action::Reveal if in_actions => Some(Msg::ToggleReveal)`), leaving the `Mode::Detail` arm in place for now
- [X] T021 [US1] Render reveal in `src/app/view/actions.rs`: `value_hint` takes the model and the row's `field_index` and returns the plaintext when `model.revealed_value(index)` answers, the mask otherwise; a revealed `Totp` row shows the code with `{remaining}s`; add a caption naming the reveal chord for the highlighted row (FR-107)
- [X] T022 [US1] Show a fetch-in-flight marker on the revealed row in `src/app/view/actions.rs` while `reveal_cancel` is set for it, matching the header spinner already used for a pending copy (spec Edge Cases)

**Checkpoint**: Reveal works fully in the field list. The detail pane is still reachable and
still passes its own tests — nothing is lost yet.

---

## Phase 4: User Story 2 - One screen for an item's fields (Priority: P2)

**Goal**: Delete the detail pane, its mode, its message, its action and its view; keep every
saved preference working. Covers FR-108 … FR-112.

**Independent Test**: Press `Ctrl+I` from the result list — nothing opens. Open preferences —
no "Show details" row. Start with a config that binds `open_detail` — every other chord
survives.

### Tests for User Story 2 ⚠️ write first, watch them fail

- [X] T023 [P] [US2] Unit test in `src/config.rs` that `Action::ALL` has 10 entries, contains no detail action, and that no action defaults to `Ctrl+I` (FR-109)
- [X] T024 [P] [US2] Unit test in `src/config.rs` that deserializing a shortcut map containing `"open_detail"` alongside `"copy_username"` (via `serde_json`) drops the unknown entry, keeps the recognized chord, and does not error (FR-110, R4)
- [X] T025 [P] [US2] Unit test in `src/config.rs` that `Preferences` with a user-set non-default chord survives `validated()` when an unknown action name was present, i.e. no silent reset to defaults (SC-005)
- [X] T026 [P] [US2] Unit test in `src/config.rs` that the `Shortcuts` newtype serializes to the same map shape it reads, so a config written by this version stays readable (contracts/config.md)
- [X] T027 [P] [US2] Unit test in `src/app/keys.rs` that `Ctrl+I` maps to `None` in every mode
- [X] T028 [P] [US2] Unit test in `src/app/view/mod.rs` that `pane()` covers every `Mode` variant with no detail pane among them

### Implementation for User Story 2

- [X] T029 [US2] Wrap `Preferences::shortcuts` in a `Shortcuts` newtype in `src/config.rs` with a hand-written `Deserialize` that reads string keys, maps known names onto `Action` and drops the rest, plus a `Serialize` that writes the same map; keep `Deref`/`DerefMut` or explicit accessors so `validated`, `chord`, `action_for` and `conflict` are unchanged in behavior (R4)
- [X] T030 [US2] Remove `Action::OpenDetail` from `src/config.rs`: the enum variant, its `ALL` entry, its `label()` arm, its `default_chord()` arm and any test referencing `Ctrl+I` as a binding
- [X] T031 [US2] Delete `Mode::Detail`, `Msg::OpenDetail`, `Model::open_detail`, `Model::in_detail` and `Model::secret_fields` from `src/core/state.rs`, collapsing `target_item`, `fetch_totp`, `toggle_reveal` and the fetch guards onto `Mode::Actions` only
- [X] T032 [US2] Remove the detail arms from `src/app/keys.rs` (`Action::OpenDetail`, the `Mode::Detail` reveal arm) and update the catch-all arm to the remaining actions
- [X] T033 [US2] Delete `src/app/view/detail.rs`, remove `pub mod detail;`, `Pane::Detail` and its `popup()` arm from `src/app/view/mod.rs`
- [X] T034 [US2] Move the detail pane's `rows()` label/value logic that the field list still needs (kind label for the header, `MASK`, `NO_VALUE`) into `src/app/view/actions.rs` and drop the rest
- [X] T035 [US2] Delete `tests/story3_detail_pane.rs`, whose behavior is now covered by `tests/story3_reveal_in_action_list.rs` (T014)
- [X] T036 [US2] Update the `Mode`/`Msg`/`Effect` doc comments in `src/core/state.rs` and `src/core/effects.rs` that still say "detail pane" to name the field list

**Checkpoint**: One per-item screen. `just check` green, no dead code, no stale doc comment.

---

## Phase 5: User Story 3 - No empty field rows (Priority: P3)

**Goal**: User-defined text fields (and unrecognized custom content) never reach the model, the
cache, the list or a copy shortcut; an item left with nothing to list says so. Covers
FR-113 … FR-117.

**Independent Test**: Parse the synthetic fixture and confirm its custom `Text` fields are
absent while `Hidden` and `Totp` ones remain; open such an item in the app and see no dash row.

### Tests for User Story 3 ⚠️ write first, watch them fail

- [X] T037 [P] [US3] Unit test in `src/pass/parse.rs` that an item with `{"name":"Nickname","content":{"Text":"fix"}}` produces no field, while a `Hidden` sibling produces a secret field and a `Totp` sibling lands in `totp_fields` (FR-113, FR-114)
- [X] T038 [P] [US3] Unit test in `src/pass/parse.rs` that an unrecognized custom content variant produces no field either (R5)
- [X] T039 [P] [US3] Unit test in `src/pass/parse.rs` that built-in fetch-on-demand members (`phone_number`, `first_name`, …) are still emitted as unstored fields (FR-115)
- [X] T040 [P] [US3] Unit test in `src/core/actions.rs` that `primary_action`, `username_action`, `url_action` and `totp_action` pick the same fields as before for an item that used to carry custom text fields (FR-116)
- [X] T041 [P] [US3] Unit test in `src/app/view/actions.rs` that an item with no listable field renders the "no copyable fields" line instead of an empty list (FR-117)
- [X] T042 [US3] Extend `tests/story2_other_fields.rs` with an item carrying a custom text field: it is not listed, not copyable, and the dedicated shortcuts still land on the right fields (FR-116)

### Implementation for User Story 3

- [X] T043 [US3] Drop `CustomFieldKind::Text` and `CustomFieldKind::Other` in `pass::parse::custom_fields` in `src/pass/parse.rs` so neither pushes a `FieldRef`, keeping `Hidden` and `Totp` unchanged; update the function's doc comment to say why
- [X] T044 [US3] Bump `CACHE_FORMAT_VERSION` from `1` to `2` in `src/model.rs` so an existing cache holding the dropped rows is discarded on load (R6)
- [X] T045 [US3] Review the snapshot diff and accept it with `INSTA_UPDATE=always cargo nextest run parse`, confirming `src/pass/snapshots/cosmic_pass__pass__parse__tests__snapshot_of_synthetic_items.snap` lost only custom text rows
- [X] T046 [US3] Render the empty-list message in `src/app/view/actions.rs` when `model.actions()` is empty, using the existing caption style (FR-117)
- [X] T047 [US3] Remove the `NO_VALUE` placeholder from `src/app/view/actions.rs` only if no built-in unstored field can still reach it; otherwise keep it and note in the comment which fields still use it (FR-115)

**Checkpoint**: All three stories independently functional.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T048 [P] Amend `specs/001-quick-access-launcher/contracts/keyboard.md`: drop the `Ctrl+I` row and the "Detail pane mode" section, fold the reveal rules into "Action list mode", and link to `specs/002-action-list-reveal/contracts/keyboard.md`
- [X] T049 [P] Amend `specs/001-quick-access-launcher/contracts/config.md`: remove `open_detail` from the action-name list and record the drop-unknown-key rule
- [X] T050 [P] Note the `CACHE_FORMAT_VERSION` 1 → 2 bump in `specs/001-quick-access-launcher/contracts/cache-format.md`
- [X] T051 [P] Mark FR-018 superseded and FR-019 relocated in `specs/001-quick-access-launcher/spec.md`, with a one-line pointer to feature 002 (the same way FR-002 records its amendment)
- [X] T052 [P] Add the reveal row (`Ctrl+R` — reveal the highlighted field) and the field-list row (`Tab`) to the Keys table in `README.md`
- [X] T053 Run `just check` and confirm fmt, clippy `-D warnings`, nextest and the 80% coverage floor all pass on the changed files
- [X] T054 Walk the manual checklist in [quickstart.md](./quickstart.md) against a signed-in `pass-cli` via `just run`
- [X] T055 Close the bd issues (`bd close <us1> <us2> <us3> <epic>`) and file follow-ups for anything deferred, such as the single-`Revealed`-enum refactor noted in research R3

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies
- **Foundational (Phase 2)**: after Setup — blocks US1 and US2 (shared renames)
- **US1 (Phase 3)**: after Foundational
- **US2 (Phase 4)**: after US1 — deleting the detail pane is only safe once reveal lives in the field list
- **US3 (Phase 5)**: after Foundational; independent of US1 and US2, can run in parallel with either
- **Polish (Phase 6)**: after the stories it documents

### Within Each User Story

- Every test task precedes the implementation task it covers; confirm the failure reason before implementing
- `ActionEntry.field_index` (T015) precedes the reducer work (T016–T019), which precedes the view (T021–T022)
- In US2, the tolerant deserializer (T029) precedes removing the variant (T030); removing the variant precedes the reducer and view deletions

### Parallel Opportunities

- T005–T013 are separate test modules in different files and can be written in parallel; T014 depends on nothing but the harness
- T023–T028 likewise, across `src/config.rs`, `src/app/keys.rs`, `src/app/view/mod.rs`
- T037–T041 likewise, across `src/pass/parse.rs`, `src/core/actions.rs`, `src/app/view/actions.rs`
- US3 (Phase 5) can proceed alongside US1/US2 — it touches `src/pass/parse.rs` and `src/model.rs`, which neither of them edits, with `src/app/view/actions.rs` the only shared file (T046/T047 after T021)
- T048–T052 are five separate documents

---

## Parallel Example: User Story 1

```bash
# Write these test modules together, then watch them fail:
Task: "field_index mapping test in src/core/actions.rs"
Task: "reveal-follows-highlight tests in src/core/state.rs"
Task: "highlight-move clears reveal test in src/core/state.rs"
Task: "one-time-code reveal tests in src/core/state.rs"
Task: "reveal chord mapping test in src/app/keys.rs"
Task: "value-line rendering tests in src/app/view/actions.rs"
```

---

## Implementation Strategy

### MVP (User Story 1 only)

1. Phase 1 Setup, Phase 2 Foundational
2. Phase 3 US1
3. **Stop and validate**: reveal works in the field list; the detail pane still exists but is
   now redundant. Shippable on its own.

### Incremental Delivery

1. Setup + Foundational → shared names in place
2. US1 → reveal in the field list (MVP)
3. US2 → detail pane gone, preferences intact
4. US3 → dead rows gone
5. Polish → contracts, README, gates

### Notes

- `[P]` tasks touch different files and have no dependency on an incomplete task
- Commit after each task or logical group; do not commit or push without explicit authorization
- `src/core` must stay free of libcosmic and IO — every reveal decision belongs in the reducer,
  every widget in `src/app/view`
- No secret may be logged, `Debug`-printed or serialized: revealed values live only in
  `view.revealed` / `view.totp`

---

## Phase 7: Convergence

- [X] T056 CRITICAL: add an acceptance-level test `tests/story2_removed_info_screen.rs` driving US2's four acceptance scenarios end to end through `Harness` — the former info-screen chord opens nothing from the result list, the preferences pane lists no detail action, a `Preferences` carrying a stale action name loads with every other chord intact, and the field list still reaches the title, kind, vault, each field and the one-time code — per Constitution III and US1/US2 acceptance scenarios (partial)
- [X] T057 Add `ron` as a dev-dependency and a test in `src/config.rs` that loads a RON shortcut map in the shape cosmic-config actually stores (bare identifier keys, e.g. `{open_detail: (modifiers: [Ctrl], key: "i"), refresh: (modifiers: [], key: "F7")}`), asserting the unknown name is dropped, the recognized chords survive, and the newtype's own output reloads — today every such test uses `serde_json` while cosmic-config reads with `ron::from_str` — per FR-110 and SC-005 (partial)
