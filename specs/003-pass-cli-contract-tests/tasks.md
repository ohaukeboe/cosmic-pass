---

description: "Task list for feature implementation"
---

# Tasks: Real `pass-cli` contract and live tests

**Input**: Design documents from `/specs/003-pass-cli-contract-tests/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/pass-cli-test-harness.md](./contracts/pass-cli-test-harness.md)

**Tests**: This feature *is* tests. There is no separate "write the test first" tier, because the
deliverable is the assertion. Constitution Principle II is satisfied differently here and the
discipline is explicit in each task: **every assertion is first written against a deliberately
wrong expectation, confirmed to fail for the expected reason, then corrected.** Tasks that
require this say `RED-FIRST`. Skipping that step makes an assertion that has never been observed
to fail, which is indistinguishable from an assertion that cannot fail.

**Organization**: grouped by user story. US1 (contract suite) is the MVP and is the only story
that joins `just check`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependencies)
- **[Story]**: US1 = contract suite, US2 = live suite, US3 = fixture shape

## Path Conventions

Single Rust crate. New code lives under `tests/`; no `src/` change is planned. Cargo compiles
every top-level `tests/*.rs` as its own test binary and does **not** compile
`tests/<dir>/mod.rs`, which is why the shared harness lives in a subdirectory.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: make the real binary reachable from a test and from the dev shell.

- [X] T001 Create `tests/support/mod.rs` declaring `pub mod real_cli;`, and an empty
      `tests/support/real_cli.rs`. Confirm with `cargo nextest list` that no new test binary
      named `support` appears — a `tests/support.rs` file would be compiled as its own binary,
      a `tests/support/` directory is not.
- [X] T002 Create `tests/pass_cli_contract.rs` containing only `mod support;` plus one
      `#[test] fn harness_compiles() {}`, and `tests/pass_cli_live.rs` containing the same.
      Confirm `cargo nextest run -E 'binary(pass_cli_contract)'` runs and passes, and that
      `cargo clippy --all-targets -- -D warnings` stays clean (unused-module warnings from a
      shared test module are the usual first failure here; resolve with `#![allow(dead_code)]`
      at the top of `tests/support/mod.rs`, with a comment saying why — each test binary uses a
      different subset of the module).
- [X] T003 [P] Add `proton-pass-cli` to the devShell in `flake.nix`: in `devShells`, add
      `nixpkgs-pass-cli.legacyPackages.${systemOf pkgs}.proton-pass-cli` to `packages`, reusing
      the input already declared at `flake.nix:19` and consumed at `flake.nix:135`. Verify with
      `nix develop -c pass-cli --version` printing `Proton Pass CLI 2.3.3`.
- [X] T004 [P] Set `COSMIC_PASS_REQUIRE_CLI = "1"` in the same devShell attrset in `flake.nix`.
      Verify with `nix develop -c sh -c 'echo $COSMIC_PASS_REQUIRE_CLI'` printing `1`.

**Checkpoint**: the dev shell provides the pinned binary and marks it required; two empty test
binaries compile.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: the harness in `tests/support/real_cli.rs`. Every story depends on it.

**⚠️ CRITICAL**: no user story work can begin until this phase is complete.

- [X] T005 Implement `ValueKind` and `RealCli` in `tests/support/real_cli.rs` per
      [data-model.md](./data-model.md): `RealCli { path: PathBuf, version: CliVersion }`.
- [X] T006 Implement `Resolution` in `tests/support/real_cli.rs` as the three-variant enum from
      data-model.md — `Found(RealCli)`, `Skip { looked_for: String }`,
      `Fail { reason: String }`. No other variant.
- [X] T007 Implement `RealCli::find() -> &'static Resolution` in `tests/support/real_cli.rs`,
      memoised in a `std::sync::OnceLock` so `--version` is parsed once per test binary
      (data-model.md, `RealCli` invariant). Resolution order: `COSMIC_PASS_CLI` env var, else
      `pass-cli` resolved on `PATH`. This is the same variable `TokioRunner::from_env` reads at
      `src/pass/runner.rs:107`.
- [X] T008 Implement the skip-versus-fail rule in `RealCli::find`:
      nothing resolved and `COSMIC_PASS_REQUIRE_CLI` unset → `Skip`;
      nothing resolved and `COSMIC_PASS_REQUIRE_CLI=1` → `Fail`;
      `--version` output that `pass::parse::parse_version` rejects → `Fail`;
      version `< cosmic_pass::core::version::TESTED_MIN` → `Fail` naming both versions;
      otherwise `Found`. A version above `TESTED_MIN` is valid and must not fail
      (research.md D5).
- [X] T009 Implement the skip macro in `tests/support/real_cli.rs`: a
      `require_cli!()` that expands to a match on `RealCli::find()` yielding `&RealCli`, or
      `eprintln!("SKIP {}: {}", env!("CARGO_CRATE_NAME"), reason)` followed by `return`, or
      `panic!(reason)`. The skip line format is fixed by
      [contracts/pass-cli-test-harness.md](./contracts/pass-cli-test-harness.md): exactly
      `SKIP <binary>: <reason>` on stderr, so it is greppable.
- [X] T010 Implement `IsolatedEnv` in `tests/support/real_cli.rs` per data-model.md: owns a
      `tempfile::TempDir`, and produces `vars` setting `HOME`, `XDG_DATA_HOME`,
      `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` all under that directory.
- [X] T011 Extend `IsolatedEnv` to neutralise the keyring reach (research.md D3, the real
      isolation risk): the child must not inherit `DBUS_SESSION_BUS_ADDRESS`, and
      `PROTON_PASS_LINUX_KEYRING` must not be `dbus` for a contract probe — note that
      `src/pass/runner.rs:134` sets `dbus` unconditionally, so `IsolatedEnv::vars` must override
      it, which works because `runner.rs:135` applies `self.env` *after* the three fixed vars.
      Clearing `DBUS_SESSION_BUS_ADDRESS` needs `Command::env_remove`, which `TokioRunner` does
      not expose; set it to an unroutable value (`unix:path=/nonexistent`) rather than adding a
      production API for a test (Principle IV).
- [X] T012 Implement `IsolatedEnv::runner(&self, cli: &RealCli) -> TokioRunner` in
      `tests/support/real_cli.rs`, building `TokioRunner::new(cli.path).with_env(self.vars())`.
      Driving probes through the production runner is deliberate (research.md D10): it means the
      tests also cover the process-group kill, null stdin and env injection production uses.
- [X] T013 Implement `IsolatedEnv::wrote_only_inside_itself(&self) -> Result<(), Vec<PathBuf>>`
      in `tests/support/real_cli.rs`, walking the temp dir and returning anything unexpected.
      Measured baseline on 2.3.3: the only file created is
      `$XDG_DATA_HOME/proton-pass-cli/.session/pass-cli.db` (research.md D3).

**Checkpoint**: harness compiles, resolves the binary, and can build an isolated runner.

---

## Phase 3: User Story 1 - Contract drift is caught by `just check` (Priority: P1) 🎯 MVP

**Goal**: the real `pass-cli`'s command surface, flag surface, id-passing rule and signed-out
error text are asserted on every `just check`, with no account and no network.

**Independent Test**: with the pinned `pass-cli` on `PATH` and no session,
`cargo nextest run -E 'binary(pass_cli_contract)'` is green; misspelling one flag in
`src/pass/backend.rs` turns exactly one assertion red (quickstart V4).

### Isolation guarantees (assert the harness before trusting it)

- [X] T014 [US1] RED-FIRST. Add `isolation::probe_is_unauthenticated` to
      `tests/pass_cli_contract.rs`: run `info --output json` through an `IsolatedEnv` runner and
      assert the failure classifies as `PassError::SignedOut` via `cosmic_pass::pass::error`.
      This is the positive assertion that proves isolation held — a probe that authenticates
      means the developer's session leaked in (contracts/pass-cli-test-harness.md, guarantee 1).
      Confirm it fails first by asserting on a different variant.
- [X] T015 [US1] Add `isolation::probe_writes_nothing_outside_its_temp_dir` to
      `tests/pass_cli_contract.rs` using `IsolatedEnv::wrote_only_inside_itself` from T013
      (guarantee 2).
- [X] T016 [US1] Add `version::reported_version_is_at_or_above_tested_min` to
      `tests/pass_cli_contract.rs`: assert `RealCli::find()` yielded `Found`, and
      `eprintln!` the version seen so a maintainer can decide to raise `TESTED_MIN`
      (FR-005, research.md D5).

### Command and flag surface (FR-006, FR-007)

Each of these parses `--help` output for the exact tokens the app sends. Token assertions only —
never a full-text snapshot, because upstream rewords help prose freely (research.md D4).

- [X] T017 [P] [US1] Add `surface::top_level_commands_exist` to `tests/pass_cli_contract.rs`:
      `--help` exits 0 and its stdout contains `login`, `logout`, `info`, `vault`, `item`.
- [X] T018 [P] [US1] Add `surface::info_accepts_output_json` to `tests/pass_cli_contract.rs`:
      `info --help` exits 0 and stdout contains `--output` and `json`.
- [X] T019 [P] [US1] Add `surface::vault_list_accepts_output_json` to
      `tests/pass_cli_contract.rs`: `vault list --help` exits 0 and stdout contains `--output`
      and `json`.
- [X] T020 [P] [US1] Add `surface::item_list_accepts_share_id_output_and_show_secrets` to
      `tests/pass_cli_contract.rs`: `item list --help` exits 0 and stdout contains `--share-id`,
      `--output`, `--show-secrets`.
- [X] T021 [P] [US1] Add `surface::item_view_accepts_share_id_item_id_and_field` to
      `tests/pass_cli_contract.rs`: `item view --help` exits 0 and stdout contains `--share-id`,
      `--item-id`, `--field`.
- [X] T022 [P] [US1] Add `surface::item_totp_accepts_share_id_item_id_and_output` to
      `tests/pass_cli_contract.rs`: `item totp --help` exits 0 and stdout contains `--share-id`,
      `--item-id`, `--output`.

### Full-argv acceptance (FR-010, as rewritten after research.md D4)

- [X] T023 [US1] Add `argv::every_command_the_app_builds_is_accepted` to
      `tests/pass_cli_contract.rs`: run the complete argv `PassCli` builds for `info`,
      `vault list`, `item list`, `item view` and `item totp` (see `src/pass/backend.rs`), and
      assert each fails as `SignedOut` — **not** as a clap usage error. A clap error means an
      argument the app sends no longer parses. Note that 2.3.3 checks authentication before
      argument validity, which is exactly why `SignedOut` is the signal for "argv accepted"
      (research.md D4).
- [X] T024 [US1] RED-FIRST. Add `argv::ids_must_use_the_equals_form` to
      `tests/pass_cli_contract.rs` (FR-008): assert the space-separated form
      `item list --share-id -leadingdash --output json` fails with stderr containing
      `unexpected argument`, and that the equals form `--share-id=-leadingdash` does **not** —
      it reaches the authentication check instead. This is the rule
      `src/pass/backend.rs:id_args` depends on for share ids that begin with `-`.

### Error classification and stdout cleanliness

- [X] T025 [US1] Add `classify::unauthenticated_stderr_is_signed_out` to
      `tests/pass_cli_contract.rs` (FR-009): feed the real stderr from an unauthenticated
      `vault list --output json` through `cosmic_pass::pass::error::classify` and assert
      `PassError::SignedOut`. Observed text on 2.3.3:
      `Error: This operation requires an authenticated client`, preceded by a coloured
      `tracing` line reading `Command is not logout there is no session`.
- [X] T026 [US1] Add `env::stdout_carries_payload_only` to `tests/pass_cli_contract.rs`
      (FR-011): assert `--version` stdout is exactly one banner line and that an
      unauthenticated `info --output json` writes nothing to stdout. Assert nothing about
      stderr quietness — `PASS_LOG_LEVEL=off` does **not** suppress error-level logging
      (research.md D4).

**Checkpoint**: US1 complete. `just test` now fails when the real `pass-cli` drifts. Validate
with quickstart V1, V2, V3, V5 and V10 before moving on.

---

## Phase 4: User Story 2 - A live account run proves the JSON shapes still parse (Priority: P2)

**Goal**: an opt-in, read-only pass against a signed-in account proves every response the app
parses still parses, including the section-field addressing upstream broke in 2.1.4.

**Independent Test**: `just test-live` against a signed-in `pass-cli` is green and prints a
per-scenario coverage table; after `pass-cli logout` it stops with a message naming
`pass-cli login` (quickstart V6, V7).

### Gating and preflight

- [X] T027 [US2] Add the double gate to every test in `tests/pass_cli_live.rs`:
      `#[ignore = "needs a signed-in pass-cli; run: just test-live"]` **and** an early return
      when `COSMIC_PASS_LIVE != "1"`, printing `SKIP pass_cli_live: set COSMIC_PASS_LIVE=1`
      (FR-004, research.md D7). Both gates are required: `--ignored` is a blunt, commonly-used
      flag, and an env gate alone would leave real-account traffic inside `just check`.
- [X] T028 [US2] Add `preflight::session_exists` to `tests/pass_cli_live.rs` (FR-018): run
      `info --output json` with the **production** environment (no `IsolatedEnv` — the live
      suite is what covers `PROTON_PASS_LINUX_KEYRING=dbus`) and, on `SignedOut`, panic with
      exactly `not signed in; run: pass-cli login`. Not an assertion dump.
- [X] T029 [US2] Add `just test-live` to `justfile`: `COSMIC_PASS_LIVE=1 cargo nextest run
      --all-features --run-ignored=only --no-tests=pass -E 'binary(pass_cli_live)' --no-capture`.
      `--no-capture` so the coverage table and latency lines actually reach the terminal.

### Scenario harness

- [X] T030 [US2] Implement `LiveScenario` in `tests/support/real_cli.rs` per data-model.md:
      `name`, `prerequisite`, and an outcome of `Covered` or `NotCovered { reason }`.
      A scenario left `Pending` at the end of a run is a harness bug and must fail the run —
      it must not silently disappear (data-model.md, state transitions).
- [X] T031 [US2] Implement the coverage report in `tests/pass_cli_live.rs`: one line per
      scenario, `covered` or `not covered: <reason>` (FR-017,
      contracts/pass-cli-test-harness.md, safety contract item 4).

### Read-only scenarios (FR-014)

- [X] T032 [P] [US2] Add `live::info_parses` to `tests/pass_cli_live.rs`: `info --output json`
      through `pass::parse::parse_account`; assert the `AccountId` is non-empty. Never print it.
- [X] T033 [P] [US2] Add `live::vault_list_parses` to `tests/pass_cli_live.rs`:
      `vault list --output json` through `parse_vaults`; assert at least one vault and that
      every `share_id` and `vault_id` is non-empty.
- [X] T034 [P] [US2] Add `live::item_list_parses_with_secrets` to `tests/pass_cli_live.rs`:
      for the first vault, `item list --share-id=<S> --output json --show-secrets` through
      `parse_items`; assert every item has a non-empty title.
- [X] T035 [P] [US2] Add `live::item_list_parses_plain` to `tests/pass_cli_live.rs`: the same
      command without `--show-secrets` through `parse_items`. The plain form has a different
      shape (`item_type` instead of `content.content`), so it needs its own scenario.
- [X] T036 [US2] Add `live::field_view_parses` to `tests/pass_cli_live.rs`: find an item whose
      parsed summary advertises a `password` field, run
      `item view --share-id=<S> --item-id=<I> --field=password` through `parse_field`, and
      assert the returned `SecretString` has non-zero length. Assert on length only; never call
      `ExposeSecret` into an assertion message (FR-020, research.md D8). No password item on the
      account → `NotCovered`.
- [X] T037 [US2] Add `live::totp_parses` to `tests/pass_cli_live.rs`: find an item with a TOTP
      field, run `item totp --share-id=<S> --item-id=<I> --output json` through `parse_totp`,
      and assert each code is exactly six ASCII digits — a character-class check, not the value.
      No TOTP item → `NotCovered`.
- [X] T038 [US2] Add `live::field_inside_a_section_parses` to `tests/pass_cli_live.rs`
      (FR-015): find an item with a field inside a named section, address it as
      `--field=<Section>.<Name>`, and assert `parse_field` succeeds. **This is the 2.1.4
      regression** and the single highest-value scenario in the suite. No section field on the
      account → `NotCovered` with that exact reason.

### Error mapping that needs a session (FR-014a)

- [X] T039 [P] [US2] Add `live::malformed_share_id_is_not_found` to `tests/pass_cli_live.rs`:
      `item list --share-id=bogus --output json` with a session present; assert `classify`
      yields `PassError::NotFound`. This moved here from the contract suite because
      unauthenticated runs return `SignedOut` first (research.md D4).
- [X] T040 [P] [US2] Add `live::missing_field_is_field_missing` to `tests/pass_cli_live.rs`:
      `item view ... --field=definitely-not-a-field` on a real item; assert `classify` yields
      `PassError::FieldMissing` from the real `Error: Field does not exist:` text.

### Reporting

- [X] T041 [US2] Add `live::latency_is_reported` to `tests/pass_cli_live.rs`: time `info`,
      `vault list` and `item list --show-secrets` and `eprintln!` the durations. **Report, not
      assert** — the figures depend on network and vault size (spec Assumptions). Compare
      against the recorded `info`/`vault list` ~0.6 s and `item list --show-secrets` ~3.6 s in
      `specs/001-quick-access-launcher/contracts/pass-cli.md`.

**Checkpoint**: US2 complete. Validate with quickstart V6 and V7.

---

## Phase 5: User Story 3 - Committed fixtures are proven to still resemble reality (Priority: P3)

**Goal**: a live run reports every key path upstream added to or removed from the JSON shapes
the committed fixtures encode.

**Independent Test**: deleting one key from a committed fixture makes the check name that exact
key path (quickstart V8).

- [X] T042 [US3] Implement `FixtureShape` in `tests/support/real_cli.rs` per data-model.md:
      `paths: BTreeSet<(String, ValueKind)>`, derived by walking a `serde_json::Value` and
      emitting one entry per container and per leaf, with array indices collapsed to `[]`.
      Worked example from data-model.md: `{"vaults":[{"share_id":"s"}]}` yields
      `("vaults", Array)`, `("vaults[]", Object)`, `("vaults[].share_id", String)`.
- [X] T043 [US3] RED-FIRST. Add unit tests for `FixtureShape` derivation in
      `tests/support/real_cli.rs` (`#[cfg(test)]` will not run there — put them in
      `tests/pass_cli_contract.rs` under a `shape` module, since they need no binary and should
      run in the default gate): the worked example above, plus nested arrays, plus `null`.
- [X] T044 [US3] Add `shape::committed_fixtures_still_match_reality` to
      `tests/pass_cli_live.rs` (FR-019): for each committed JSON fixture under
      `tests/fixtures/pass-cli/captured/` — `vault-list.json`, `info.json`,
      `item-list-share-1.json`, `item-list-share-2.json`, `item-list-plain-share-1.json`,
      `item-list-plain-share-2.json` — derive a `FixtureShape` and compare it against one
      derived from the corresponding fresh command output.
- [X] T045 [US3] Report shape differences as two sorted lists — paths only in fresh output
      (upstream added), paths only in the fixture (upstream removed) — never as values
      (FR-020, data-model.md validation rules). A fixture whose vault has no item of a kind
      present in the fixture is a false "removed"; scope the comparison to key paths reachable
      from the kinds actually present in both.

**Checkpoint**: all three stories independently functional.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T046 [P] Correct `specs/001-quick-access-launcher/contracts/pass-cli.md`, amendment 1
      from [contracts/pass-cli-test-harness.md](./contracts/pass-cli-test-harness.md): the
      "Error format" section says a coloured tracing line may come first "with `PASS_LOG_LEVEL`
      unset". It comes first regardless; `PASS_LOG_LEVEL=off` does not silence error-level
      logging on 2.3.3.
- [X] T047 [P] Correct `specs/001-quick-access-launcher/contracts/pass-cli.md`, amendment 2:
      the "Error mapping" table is missing the `LocalData` rule that `src/pass/error.rs:70`
      implements (`file is not a database`, `logout --force`, `failed to initialize database`),
      which sits **between** the `SignedOut` and `FieldMissing` rules, and is missing
      `forcing logout` from the `SignedOut` row. The table is the documented order the tests
      assert against, so it must match the code.
- [X] T048 Update the coverage table in
      `specs/003-pass-cli-contract-tests/contracts/pass-cli-test-harness.md` to name the actual
      test function for each clause now that the functions exist (SC-001). Every clause of the
      consumed contract must read `contract`, `live`, `already covered`, or `manual` with a
      reason.
- [X] T049 Extend `scripts/leak-scan.sh` with a live step (FR-020). The existing scan greps for
      the `SECRET-FIXTURE-` marker, which real account secrets do not carry, so a marker grep
      proves nothing about the live suite. Instead assert the live suite's output vocabulary:
      run it with `COSMIC_PASS_LIVE=1`, capture the log, and fail if any line fails to match the
      fixed set of shapes the suite is allowed to print (scenario name, `covered` /
      `not covered: <reason>`, a duration line, a version line, nextest's own output). Gate the
      whole step on `COSMIC_PASS_LIVE=1` and add it to the existing `skipped` array otherwise,
      so the script keeps its existing exit-code contract (0 clean, 1 leak, 2 inconclusive).
- [X] T050 [P] Document the live suite in `README.md`, `CLAUDE.md` and `AGENTS.md` under
      Build & Test (FR-023): `just test-live`, what it requires (a signed-in `pass-cli`, the
      three item kinds), that it is read-only, and that `COSMIC_PASS_CLI` points both suites at
      a specific binary. `CLAUDE.md` and `AGENTS.md` are independent files — mirror the edit
      across both.
- [X] T051 Run `just check` inside `nix develop`. Confirm green, confirm no
      `SKIP pass_cli_contract:` line appears (with `COSMIC_PASS_REQUIRE_CLI=1` set by the shell
      a skip would be a bug), and confirm line coverage stays at or above 80% (SC-007). If the
      live binary's uncovered lines threaten the floor, exclude it via the
      `--ignore-filename-regex` in the `cov` recipe and cite plan.md Complexity Tracking row 3.
- [X] T052 Run quickstart V1–V10 from [quickstart.md](./quickstart.md) and record the outcome of
      each, including which live scenarios reported `not covered` on the account used.
- [X] T053 Demonstrate SC-002 (quickstart V4): temporarily apply each of the three mutations to
      `src/pass/backend.rs` in turn, confirm **exactly one** contract assertion fails per
      mutation and that its message names the contract clause, then revert. Broad collateral
      failure means the assertions are coupled and must be tightened before this task closes.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies.
- **Foundational (Phase 2)**: depends on Phase 1. **Blocks all user stories.**
- **US1 (Phase 3)**: depends on Phase 2 only.
- **US2 (Phase 4)**: depends on Phase 2 only. Independent of US1.
- **US3 (Phase 5)**: depends on Phase 2, and on T027/T028 from US2 for the live gate — US3's
  check runs inside the live binary. This is the one cross-story dependency in the feature and
  it is structural, not incidental.
- **Polish (Phase 6)**: T046, T047 and T050 depend on nothing and can be done at any time;
  T048, T049, T051, T052 and T053 depend on the stories they verify.

### Within Each Story

- Isolation assertions (T014, T015) before any other contract probe — an unproven `IsolatedEnv`
  makes every later probe's result meaningless.
- Scenario harness (T030, T031) before the live scenarios that report through it.
- `FixtureShape` (T042) and its unit tests (T043) before the live comparison (T044).

### Parallel Opportunities

- T003 and T004 are both `flake.nix` edits to the same attrset — do them in one pass despite the
  `[P]` on each; they are marked parallel only in the sense of being independent of T001/T002.
- T017–T022 are six independent `--help` probes in the same file: write them together, they
  share no state.
- T032–T035 and T039–T040 are independent live scenarios.
- T046, T047 and T050 are documentation edits in files no other task touches.

---

## Parallel Example: User Story 1

```bash
# The six help-surface probes are independent of each other (T017-T022).
# Verify them as a group:
cargo nextest run -E 'binary(pass_cli_contract) and test(surface::)'

# Then the argv and classification probes:
cargo nextest run -E 'binary(pass_cli_contract) and (test(argv::) or test(classify::))'
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Phase 1 Setup — the dev shell provides the binary.
2. Phase 2 Foundational — the harness. **Blocks everything.**
3. Phase 3 US1 — the contract suite.
4. **STOP and VALIDATE**: quickstart V1, V2, V3, V5, V10, and the SC-002 demonstration (T053).
5. This alone delivers the feature's core value: drift now fails `just check` with no account,
   no network and no credentials.

### Incremental Delivery

1. Setup + Foundational → harness ready.
2. US1 → automated drift detection in the default gate. **MVP.**
3. US2 → the opt-in live suite, run before a release or after an upstream bump.
4. US3 → fixture-shape reporting, inside the live suite.

### Notes

- `[P]` = different files, no dependencies.
- Commit after each task or logical group. Do not commit or push without explicit
  authorization (constitution, Development Workflow).
- The whole feature touches only `tests/`, `flake.nix`, `justfile`, `scripts/leak-scan.sh` and
  documentation. **No `src/` change is planned.** A task that appears to need one is a signal to
  stop and reconsider — except T053, which mutates `src/pass/backend.rs` deliberately and
  reverts.

---

## Implementation notes (2026-09-21)

Four things ended up different from the plan. Each is deliberate.

1. **T009 is a function, not a macro.** `require_cli!()` would have needed `#[macro_export]` and
   crate-root ordering to be usable from a test binary's submodules. `real_cli::or_skip() ->
   Option<&'static RealCli>` reads as ordinary control flow — `let Some(cli) = or_skip() else
   { return };` — with nothing to reason about. `or_skip_live()` is the live suite's variant,
   adding the `COSMIC_PASS_LIVE` gate in front.

2. **`src/pass/backend.rs` changed after all**, against the plan's "no `src/` change".
   The contract suite originally retyped the app's argv, and a mutation test showed the
   consequence: changing `--show-secrets` in `backend.rs` left the suite green, because the
   suite was asserting its own copy. Extracting `pub mod argv` and having both the backend and
   the tests source argv from it is what makes SC-002 hold. No behaviour changed; the argv are
   byte-for-byte what they were.

3. **The fixture-shape check is asymmetric.** `capture-fixtures` keeps only a couple of items
   per kind, so which optional sub-objects a fixture carries is partly chance. A path the
   fixture has and fresh output lacks means upstream removed a field: red. A path fresh output
   has and the fixture lacks is usually sampling: reported, green. The first live run reported
   31 such added paths (`Login.passkeys[]`, `content.platform_specific`,
   `extra_fields[].content.Totp`, `Custom.sections[].section_fields[].content.Text`) and no
   removals.

4. **The leak scan checks vocabulary, not markers** (T049). The existing scan greps for
   `SECRET-FIXTURE-`, which a real account's secrets do not carry, so it could never have said
   anything about the live suite. The new step instead fails when the live suite prints a line
   outside the fixed set it is allowed to print.

### Not verified

- **Quickstart V7** (the live suite refusing to run with no session) was not exercised: it would
  have meant logging the developer out of their own account. The path is `live()` in
  `tests/pass_cli_live.rs`, which panics with `not signed in; run: pass-cli login`.
- **`Locked`, `Network` and `LocalData` classification** remain manual, as planned — each needs
  an induced failure the suites cannot produce. They are listed as manual in the coverage table.

---

## Phase 7: Convergence

Appended by `/speckit-converge` on 2026-09-21 after assessing the implemented tree against
`spec.md`, `plan.md` and this file. Two gaps remain; neither is a constitution violation.

- [X] T054 Give every raw probe a timeout that kills the child, per FR-013 and research.md D10
      (`partial`). `RawOutput::capture` in `tests/support/real_cli.rs` and `probe_version` in the
      same file both call blocking `std::process::Command::output()`, which waits forever. Eight
      contract probes run through `capture` — the six `surface::*` `--help` probes,
      `argv::ids_must_use_the_equals_form` and `env::stdout_carries_payload_only` — so a
      `pass-cli` that hangs wedges the test binary instead of failing it, which is the spec's
      "A contract probe hangs" edge case. D10 fixed the figure: a flat 10 s for `--help`-style
      probes. Keep `RawOutput` synchronous if that reads better (spawn, wait with a deadline,
      `kill` on expiry), or move it onto `TokioRunner`'s path; either way the child must be
      terminated, not merely abandoned. RED-FIRST: point a probe at a script that sleeps past
      the deadline and confirm the suite fails with a timeout message before wiring the real
      binary back in.
- [X] T055 Shape-check the two share-2 fixtures, per FR-019 and US3/AC1 (`partial`).
      `committed_fixtures_still_match_reality` in `tests/pass_cli_live.rs` compares `info.json`,
      `vault-list.json`, `item-list-share-1.json` and `item-list-plain-share-1.json` only;
      `item-list-share-2.json` and `item-list-plain-share-2.json` — both named in T044 — are
      compared against nothing, so they can go stale unobserved while `just test-live` stays
      green. Compare them against the second vault in `vault list` output, reusing `compare` and
      `drop_absent_kinds`. An account with only one vault has no second listing to compare
      against: report `LiveScenario::not_covered("fixture-shape-share-2", ...)` with that reason
      (FR-017) rather than skipping silently. Update the `fixtures compared: N` line and the
      "Known limit of the fixture-shape check" note in
      `contracts/pass-cli-test-harness.md` if the count or the caveat changes.

## Convergence notes (2026-09-21)

T054 and T055 close the two gaps `/speckit-converge` found.

1. **`RawOutput::try_capture` replaces `Command::output`** (T054). The raw probes — the six
   `--help` probes, `ids_must_use_the_equals_form`, `stdout_carries_payload_only` and version
   resolution — waited forever, so a hung `pass-cli` would have wedged the test binary rather
   than failing it, against FR-013 and research.md D10. `try_capture` drains both pipes on their
   own threads (a child blocked writing to a full pipe looks exactly like a hang), polls
   `try_wait` until `PROBE_TIMEOUT`, then kills and reaps.
   `timeout::a_hung_probe_is_killed_rather_than_waited_on` asserts it against a
   `sh -c 'sleep 2; : > marker'` stand-in: the error text, the elapsed time, and — the assertion
   that matters — that the marker never appears. RED confirmed by removing the `kill` call,
   which makes exactly that assertion fail with `the child outlived the probe that spawned it`.

2. **The `-share-2` fixtures are compared against the second vault** (T055). Both fixture and
   fresh output are per-vault, so the first vault's listing could never have checked them. An
   account with one vault reports
   `SCENARIO fixture-shape-share-2: not covered: the account has one vault, ...` rather than
   passing silently.

### Not verified (convergence)

- **T055's live behaviour.** `just test-live` could not run on 2026-09-21: the developer's
  session was *locked*, not signed out, so `live()` panicked with
  `pass-cli is not usable: the Proton Pass session is locked` before any scenario ran. Unlocking
  needs the account passphrase. The code compiles and passes `clippy -D warnings`; the
  `fixtures compared: 6` count and the `fixture-shape-share-2` line have not been observed.
  Run `just test-live` after unlocking the session to close this out.
