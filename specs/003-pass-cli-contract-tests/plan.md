# Implementation Plan: Real `pass-cli` contract and live tests

**Branch**: `003-pass-cli-contract-tests` | **Date**: 2026-09-21 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/003-pass-cli-contract-tests/spec.md`

## Summary

Every test today drives `tests/fixtures/fake-pass-cli`, so nothing proves the app still matches
the real `pass-cli` — a tool with no stability policy that has broken the app's assumptions
inside patch releases. Add two integration test binaries that drive the real binary:
`tests/pass_cli_contract.rs`, which needs no Proton account and joins the default `just check`
gate, and `tests/pass_cli_live.rs`, an opt-in read-only suite against a signed-in account. A
shared `tests/support/real_cli.rs` resolves the binary, checks its version against
`core::version::TESTED_MIN`, and builds a throwaway home so no probe can touch the developer's
session. The flake devShell gains the pinned `proton-pass-cli` and marks it required, so a
broken pin fails rather than silently disabling the feature.

Probing 2.3.3 during Phase 0 settled the shape (see [research.md](./research.md)): the offline
half is `--help` token assertions plus real unauthenticated stderr fed through the app's own
`classify`, and one clause the spec assumed was offline-reachable (`NotFound` from a malformed
share id) turned out to require a session and moved to the live suite.

## Technical Context

**Language/Version**: Rust, edition 2024, `rust-version = "1.93"`

**Primary Dependencies**: existing only — `tokio` (multi-thread, process, time), `tokio-util`
(`CancellationToken`), `secrecy`, `serde_json`, `tempfile` (already a runtime dependency).
No new crate.

**Storage**: N/A. Tests read `tests/fixtures/pass-cli/captured/*.json` and write only inside
per-test `TempDir`s.

**Testing**: `cargo nextest` via `just test` / `just check`; a new `just test-live` recipe for
the opt-in suite. Coverage via `cargo llvm-cov` (`just cov`, 80% floor).

**Target Platform**: Linux desktop (COSMIC). The contract suite needs no compositor, no network,
no D-Bus.

**Project Type**: Single Rust crate; this feature touches only `tests/`, `flake.nix`, `justfile`,
`scripts/leak-scan.sh` and documentation.

**Performance Goals**: contract suite under 30 s wall clock (SC-003). Measured headroom is large:
each unauthenticated probe on 2.3.3 returns in roughly 10 ms.

**Constraints**: no secret value may be printed, asserted on, written, or passed in argv
(FR-020, FR-021); the live suite must be read-only (FR-016); no contract probe may reach the
developer's real session or Secret Service (FR-012).

**Scale/Scope**: about 20 contract assertions and 10 live scenarios, plus one fixture-shape
check per committed captured fixture (currently 7 files).

## Constitution Check

*GATE: checked before Phase 0 and re-checked after Phase 1 design. Result: **PASS**, with one
deviation recorded in Complexity Tracking.*

| Principle | Assessment |
|---|---|
| **I. Code quality** | Test code obeys the same `just fmt` / `just lint -D warnings` gates. No suppressions planned. No credential enters source: the live suite reads whatever account the developer is signed into and asserts only on shape. |
| **II. Test-first** | This feature *is* tests. The Red→Green discipline still applies to the harness: each probe is written to fail first against a deliberately wrong expectation (a misspelled flag) before the real expectation goes in, which is also how SC-002 is demonstrated. |
| **III. Layered coverage** | Squarely the "integration tests MUST cover boundaries: external services … public CLI surfaces" clause — this is the boundary test the project was missing. Determinism: the contract suite is deterministic given a pinned binary and touches no network; the live suite is non-deterministic by nature and is therefore kept out of the gate entirely. Each user story has its own suite or scenario set. |
| **IV. Simplicity** | No new dependency, no new crate, no cargo feature. Two test binaries and one shared module. `TokioRunner` is reused rather than reimplemented, so the tests exercise the production spawn path. |
| **V. Contracts and docs** | `contracts/pass-cli-test-harness.md` documents the harness contract and, in its coverage table, names the checker for every clause of the consumed interface. Two corrections to the consumed contract found during Phase 0 are listed there and are part of this change. `CLAUDE.md`, `AGENTS.md` and `README` get the `just test-live` instructions (FR-023). |

**Gate on dynamic skipping (Principle II: "Tests MUST NOT be … skipped")**: the contract suite
can return early when no `pass-cli` is present. Justified and bounded in Complexity Tracking
below — the escape hatch is closed everywhere the project controls the environment.

**Post-design re-check**: the Phase 1 artifacts introduce no production code, no new dependency
and no new abstraction beyond the three harness types in [data-model.md](./data-model.md), each
of which exists to keep a rule (discovery, isolation, shape comparison) in one place instead of
repeated per test. Gate still passes; no new deviation.

## Project Structure

### Documentation (this feature)

```text
specs/003-pass-cli-contract-tests/
├── plan.md                              # This file
├── spec.md                              # Feature specification
├── research.md                          # Phase 0 output — D1..D10, probed against 2.3.3
├── data-model.md                        # Phase 1 output — harness types
├── quickstart.md                        # Phase 1 output — how to run and validate
├── contracts/
│   └── pass-cli-test-harness.md         # Phase 1 output — harness + coverage table
└── tasks.md                             # Phase 2 output (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
tests/
├── support/
│   └── real_cli.rs          # NEW  RealCli, Resolution, IsolatedEnv, FixtureShape
├── pass_cli_contract.rs     # NEW  default gate; real binary, no account
├── pass_cli_live.rs         # NEW  opt-in; real binary, signed-in account, read-only
├── pass_cli_integration.rs  #      unchanged; fake-pass-cli boundary suite
├── fixtures/
│   └── pass-cli/captured/   #      read by the fixture-shape check
└── ...                      #      story tests, cache_integration, search_bench

src/
├── pass/{runner.rs,backend.rs,parse.rs,error.rs}   # reused, unchanged
└── core/version.rs                                 # TESTED_MIN, reused, unchanged

flake.nix                    # devShell gains proton-pass-cli + COSMIC_PASS_REQUIRE_CLI=1
justfile                     # gains `test-live`
scripts/leak-scan.sh         # extended to cover live-run output
specs/001-quick-access-launcher/contracts/pass-cli.md   # two corrections from Phase 0
README.md, CLAUDE.md, AGENTS.md                          # how to run the live suite
```

**Structure Decision**: the existing single-crate layout is kept unchanged. New tests go in
`tests/` alongside the other integration binaries; `tests/support/` is a new shared module
directory (cargo treats a subdirectory module as shared code, not as its own test binary). No
production source changes, which is what keeps this feature's risk to the test tree.

## Phase 0 findings that changed the design

Recorded here because they alter the spec, not just the implementation. Full detail in
[research.md](./research.md).

1. **`NotFound` is not reachable offline.** 2.3.3 checks authentication before argument
   validity, so `item list --share-id=bogus` returns `requires an authenticated client`, not the
   `idformat` error in the captured fixtures. FR-010 was rewritten; the rule moved to the live
   suite as FR-014a.
2. **`PASS_LOG_LEVEL=off` does not silence error-level logging.** A coloured `tracing` `ERROR`
   line still precedes the `Error:` line. Harmless for `classify`, but the consumed contract
   implies otherwise and is corrected.
3. **The keyring is the real isolation risk.** Production sets
   `PROTON_PASS_LINUX_KEYRING=dbus`, which stores the session key in the Secret Service — a bus,
   not a directory, so a temp `HOME` does not isolate it. Contract probes drop the bus address
   from the child environment and assert they end up unauthenticated.
4. **The consumed contract's error table is missing `LocalData`.** The code has a rule the
   document does not; the table is the order the tests assert against, so it is corrected.

## Implementation phases (for `/speckit-tasks`)

Ordered so each step is independently verifiable.

1. **Harness** — `tests/support/real_cli.rs`: `RealCli::find`, `Resolution`, version floor check,
   `IsolatedEnv`. Verified by a contract test that asserts a probe ends unauthenticated and
   writes nothing outside its temp directory.
2. **Story 1, contract suite** — `tests/pass_cli_contract.rs`: version, `--help` token
   assertions per command, full-argv acceptance, `--flag=VALUE` rule, `SignedOut` classification,
   stdout cleanliness.
3. **Environment** — `flake.nix` devShell (`proton-pass-cli`, `COSMIC_PASS_REQUIRE_CLI=1`);
   confirm `just check` runs the suite inside the shell and skips outside it.
4. **Story 2, live suite** — `tests/pass_cli_live.rs`: preflight, the read-only scenarios, the
   section-field regression, the coverage report; `just test-live`.
5. **Story 3, fixture shape** — `FixtureShape` plus the comparison test, inside the live suite.
6. **Docs and safety** — the two consumed-contract corrections, the coverage table kept in sync,
   `scripts/leak-scan.sh` extension, `README`/`CLAUDE.md`/`AGENTS.md` updates.
7. **Demonstrate SC-002** — temporarily misspell one flag in `src/pass/backend.rs`, confirm
   exactly one contract assertion fails, revert.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| Principle II: a test may skip at runtime (`Resolution::Skip`) when no `pass-cli` is found | The project supports contributors without Nix, and `pass-cli` is not a cargo dependency that `cargo test` can fetch. Without the escape hatch, a bare `cargo test` on such a machine is permanently red, which trains people to ignore red. | *Always fail*: rejected, see above. *Always skip*: rejected — a broken flake pin would silently disable the whole feature, the exact failure this feature exists to prevent. The hatch is bounded: `COSMIC_PASS_REQUIRE_CLI=1` in the devShell turns it into a failure everywhere the project controls the environment, and every skip prints a greppable `SKIP …` line. |
| Principle III: the live suite is non-deterministic (real network, real account, real clock for TOTP) | Only a real account can prove the JSON shapes and the section-field addressing that upstream broke in 2.1.4 still parse. No fixture can prove that, because fixtures are exactly the thing that goes stale. | *Fold it into the default gate*: rejected — it would make `just check` depend on credentials and network. It is kept out of every gate, behind both `#[ignore]` and `COSMIC_PASS_LIVE=1`, so a flaky live result can never turn a merge red. |
| Coverage: the live suite's code runs only on demand, so its lines do not count toward `just cov` | It cannot run in the coverage job without credentials. | The contract suite and `tests/support/` do run under coverage; the live binary is small, branch-light, and carries no logic that is not exercised when it does run. If the 80% floor is threatened, the live binary is excluded explicitly with this row as the justification, per the constitution's Quality Standards clause. |
