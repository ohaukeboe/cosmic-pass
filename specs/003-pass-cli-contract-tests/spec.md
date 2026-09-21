# Feature Specification: Real `pass-cli` contract and live tests

**Feature Branch**: `003-pass-cli-contract-tests`

**Created**: 2026-09-21

**Status**: Draft

**Input**: User description: "Add tests using the pass-cli. This is for ensuring that it works with the current version of pass-cli which does not have a stable api"

## Context

Every automated test the project has today runs against `tests/fixtures/fake-pass-cli`, a
hand-written stand-in. That proves the app drives *a* command surface correctly; it proves
nothing about the surface the real `pass-cli` exposes this week. Upstream publishes no
stability policy and has already broken the app's assumptions inside patch releases:

- 2.1.4 made the section name part of a field's address (`--field=Section.Name`).
- 2.2.2 renamed `session lock` and reused the old name for something else.
- 2.2.4 removed a command outright.

The consumed surface is written down in `specs/001-quick-access-launcher/contracts/pass-cli.md`
and was last confirmed by hand against 2.3.3 on 2026-09-17. Nothing re-checks it. When the
pinned `proton-pass-cli` in `flake.nix` is bumped, or a user runs a newer `pass-cli` from their
own `PATH`, a break is discovered by a user, not by a test.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Contract drift is caught by `just check` (Priority: P1)

A maintainer bumps the pinned `proton-pass-cli` flake input, or edits the app's argv building,
and runs `just check`. If the real `pass-cli` no longer offers a command, subcommand, or flag
the app sends — or no longer produces the error text the app classifies on — the suite fails
and names the exact contract clause that broke.

**Why this priority**: This is the whole point of the feature and the only part that can run
unattended. It needs no Proton account, no network, and no credentials, so it can sit inside
the default pre-merge gate.

**Independent Test**: With the pinned `pass-cli` on `PATH` and no signed-in session, run
`just test`. Every contract assertion runs against the real binary and passes. Deliberately
mutating one asserted flag name in the test makes exactly that assertion fail.

**Acceptance Scenarios**:

1. **Given** the pinned `pass-cli` is on `PATH`, **When** the contract suite runs, **Then**
   the reported version parses and is at or above `core::version::TESTED_MIN`.
2. **Given** the pinned `pass-cli` is on `PATH`, **When** the contract suite runs, **Then**
   every command, subcommand and flag named in the consumed-interface contract is still
   accepted by the binary.
3. **Given** an isolated empty home with no `pass-cli` session, **When** the suite runs a
   command that needs authentication, **Then** the real stderr classifies as `SignedOut`.
4. **Given** an isolated empty home, **When** the suite runs the complete argv the app builds
   for each command, **Then** the binary accepts the argv (it fails on authentication, not on
   argument parsing).
5. **Given** an isolated empty home, **When** the suite passes an id with the space-separated
   form instead of `--flag=VALUE`, **Then** the binary still rejects it as an unexpected
   argument, confirming the rule the app's argv building depends on.

---

### User Story 2 - A live account run proves the JSON shapes still parse (Priority: P2)

Before a release, or after an upstream bump that the contract suite passed, a maintainer runs
one opt-in command against their own signed-in `pass-cli`. It exercises the real end-to-end
path — list vaults, list items, read a field, read a TOTP code, read a field inside a section
— and fails if any response no longer parses into the app's model.

**Why this priority**: Highest fidelity, but it needs credentials, network and a human, so it
can never gate a merge. It is the second line of defence behind Story 1.

**Independent Test**: With a signed-in `pass-cli`, run the live suite and watch every scenario
pass. With `pass-cli logout` first, the suite refuses to run and says why rather than failing
obscurely.

**Acceptance Scenarios**:

1. **Given** a signed-in `pass-cli`, **When** the live suite runs, **Then** `info`,
   `vault list`, `item list` (with and without `--show-secrets`), `item view --field` and
   `item totp` all produce output the app's parsers accept.
2. **Given** an item that stores a field inside a named section, **When** the live suite
   addresses it as `Section.Name`, **Then** the value is returned.
3. **Given** the live suite runs, **When** it reports results, **Then** no secret value
   appears in any assertion message, log line or test output.
4. **Given** no signed-in session, **When** the live suite is invoked, **Then** it stops with
   a message naming `pass-cli login` instead of reporting an assertion failure.
5. **Given** the live suite runs, **When** it finishes, **Then** it has only read; it has
   created, modified and deleted nothing in the user's vaults.

---

### User Story 3 - Committed fixtures are proven to still resemble reality (Priority: P3)

The redacted fixtures under `tests/fixtures/pass-cli/captured/` are the basis of the parser
tests. When upstream adds, renames or removes a JSON field, those fixtures silently become
fiction. A live run compares the *shape* of fresh output against the committed fixtures and
reports every key path that appeared or disappeared.

**Why this priority**: Valuable, but it only reports staleness in test data, not a user-facing
break, and it depends on Story 2's live access.

**Independent Test**: Run the shape check against a signed-in `pass-cli`; it passes on
unchanged fixtures. Hand-editing a key out of a committed fixture makes the check name that
key path.

**Acceptance Scenarios**:

1. **Given** a signed-in `pass-cli`, **When** the shape check runs, **Then** the set of key
   paths in fresh redacted output equals the set in the committed fixtures, or the difference
   is reported key path by key path.
2. **Given** the shape check runs, **When** it compares, **Then** it compares key paths and
   value kinds only; it never writes a real value to disk or to the test log.

---

### Edge Cases

- **No `pass-cli` on `PATH` and not inside the dev shell**: the contract suite reports a skip
  naming the binary it looked for, so a contributor without Nix still gets a green
  `cargo test`.
- **No `pass-cli` on `PATH` while inside the dev shell**: this means the flake is broken, so
  the suite fails rather than skipping.
- **Installed `pass-cli` is older than `TESTED_MIN`**: the contract suite fails and names both
  versions, because the pinned toolchain is then not the one the app claims to support.
- **Installed `pass-cli` is newer than `TESTED_MIN`**: the suite passes but records the
  version it saw, so a maintainer can raise the tested floor deliberately.
- **A contract probe hangs**: every probe carries a timeout and the process is killed, so a
  hung binary fails the suite instead of wedging CI.
- **The user's real session**: no contract probe may touch it. Each probe runs with an
  isolated, throwaway home directory.
- **The signed-in account has no TOTP item, or no section field**: the live suite reports
  which scenario it could not exercise instead of passing silently or failing.

## Requirements *(mandatory)*

### Functional Requirements

#### Discovery and gating

- **FR-001**: The test suite MUST locate the `pass-cli` under test from an explicit
  environment override first, then from `PATH`.
- **FR-002**: When no `pass-cli` is found, the contract suite MUST skip with a message naming
  what it looked for and how to provide it — except when the environment marks the binary as
  required, in which case it MUST fail.
- **FR-003**: The project's reproducible dev environment MUST provide the pinned `pass-cli`
  and MUST mark it as required, so `just check` inside the dev shell always exercises the
  contract suite.
- **FR-004**: The live suite MUST NOT run as part of the default test command. It MUST run
  only on explicit opt-in.

#### Contract coverage (no account required)

- **FR-005**: The contract suite MUST assert that the reported version parses with the app's
  own version parser and is at or above `TESTED_MIN`.
- **FR-006**: The contract suite MUST assert that every command and subcommand the app invokes
  (`--version`, `info`, `vault list`, `item list`, `item view`, `item totp`, `login`) is still
  accepted by the binary.
- **FR-007**: The contract suite MUST assert that every flag the app sends (`--output`,
  `--share-id`, `--item-id`, `--field`, `--show-secrets`) is still accepted on the subcommand
  the app sends it on.
- **FR-008**: The contract suite MUST assert that ids passed as `--flag=VALUE` are accepted
  and that the space-separated form is rejected, because the app depends on that rule for ids
  that begin with `-`.
- **FR-009**: The contract suite MUST assert that real stderr from an unauthenticated run
  classifies as `SignedOut` through the app's own classifier.
- **FR-010**: The contract suite MUST assert that the complete argv the app builds for each
  command is accepted by the binary — that is, that an unauthenticated run fails on
  authentication rather than on argument parsing. The `NotFound` classification rule is not
  reachable without an account (see `research.md` D4) and is covered by the live suite
  instead, under FR-014a.
- **FR-011**: The contract suite MUST assert that the environment the app sets
  (`PASS_LOG_LEVEL=off`, `PROTON_PASS_NO_UPDATE_CHECK=1`) is still accepted and still keeps
  non-payload chatter off stdout.
- **FR-012**: Every contract probe MUST run against an isolated throwaway home directory and
  MUST NOT read or modify the developer's real `pass-cli` session or store.
- **FR-013**: Every contract probe MUST carry a timeout and MUST terminate the process on
  expiry.

#### Live coverage (signed-in account required)

- **FR-014**: The live suite MUST parse real `info`, `vault list`, `item list` (both with and
  without `--show-secrets`), `item view --field` and `item totp` output with the app's own
  parsers.
- **FR-014a**: The live suite MUST assert that real stderr from a malformed share id and from a
  missing item classifies as `NotFound` through the app's own classifier.
- **FR-015**: The live suite MUST exercise addressing a field inside a named section, the
  case upstream broke in 2.1.4.
- **FR-016**: The live suite MUST be read-only against the user's vaults.
- **FR-017**: When a scenario cannot be exercised because the account lacks the required item
  kind, the live suite MUST report which scenario was not covered rather than passing
  silently.
- **FR-018**: When invoked without a signed-in session, the live suite MUST stop with a
  message naming the sign-in command.

#### Fixture freshness

- **FR-019**: A live check MUST compare the key paths and value kinds of fresh redacted output
  against the committed captured fixtures and report each added or removed key path.

#### Secret safety

- **FR-020**: No test in this feature may print, log, assert on, or write to disk a secret
  value obtained from a real account. Assertions MUST be on shape, presence and length only.
- **FR-021**: No test in this feature may pass a secret value in argv.

#### Documentation

- **FR-022**: The consumed-interface contract MUST record which of its clauses are now checked
  automatically and which remain manual.
- **FR-023**: The project's Build & Test documentation MUST state how to run the live suite
  and what it requires.

### Key Entities

- **CLI under test**: the concrete `pass-cli` binary a run resolved, plus the version it
  reports. Every probe in a run uses the same one.
- **Contract probe**: one invocation of the real binary plus the assertion it supports, traced
  back to a named clause of the consumed-interface contract.
- **Isolated environment**: a throwaway home and config directory, plus the env the app sets,
  in which a contract probe runs so the developer's session is untouched.
- **Live scenario**: one end-to-end read against a signed-in account, with the app parser it
  feeds and the prerequisite item kind it needs.
- **Fixture shape**: the set of key paths and value kinds of a captured fixture, compared
  against fresh output.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every clause of the consumed-interface contract that can be checked without an
  account is checked by an automated assertion; the contract document names the checker for
  each clause and names the remainder as manual.
- **SC-002**: Introducing a single wrong flag name, subcommand name, or id-passing form into
  the app's argv building makes at least one contract assertion fail.
- **SC-003**: The contract suite adds under 30 seconds to `just check` on a developer machine.
- **SC-004**: The full default suite stays green, with no network access and no Proton
  account, on a machine that has never run `pass-cli login`.
- **SC-005**: The live suite completes a full read-only pass against a real account and
  reports, per scenario, covered or not-covered with a reason.
- **SC-006**: A scan of everything the suites write and print finds no secret value.
- **SC-007**: Line coverage on changed code stays at or above 80%.

## Assumptions

- The pinned `proton-pass-cli` from the `nixpkgs-pass-cli` flake input is the reference
  implementation; a `pass-cli` a user installed themselves still wins on `PATH`, and the suite
  tests whichever one it resolves.
- `pass-cli` keeps its store and session under the home directory, so overriding home is
  enough to isolate a probe. The isolation itself is asserted, not assumed: a probe that
  reaches an authenticated state proves the isolation failed and fails the test.
- The live suite runs against whatever account the developer is already signed into; the
  feature does not provision a test account or test data.
- Latency figures recorded in the contract are reported, not asserted, because they depend on
  network and vault size.
- CI does not exist yet; "always runs" means "runs in `just check` inside the dev shell".
