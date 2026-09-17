# cosmic-pass Constitution

## Core Principles

### I. Code Quality Is Non-Negotiable

- All code MUST pass the project's configured formatter and linter with zero errors and zero
  warnings before merge. Suppressions MUST be local, and each MUST carry a comment stating why.
- Static type checking MUST be enabled wherever the language supports it. Implicit `any`
  (or the language equivalent) is forbidden in production code.
- Names MUST describe intent. Functions MUST do one thing. Dead code, commented-out code, and
  debug output MUST NOT be merged.
- Errors MUST be handled or propagated explicitly. Silently swallowed errors are forbidden.
- Secrets and credentials MUST NOT appear in source, tests, or version control.

**Rationale**: Consistent, automatically enforced quality removes style debate from review and
catches whole classes of defects before they reach runtime.

### II. Test-First Development (NON-NEGOTIABLE)

- Every behavior change MUST start with a failing test that expresses the requirement.
- The Red → Green → Refactor cycle MUST be followed: write test, confirm it fails for the
  expected reason, implement the minimum to pass, then refactor with tests green.
- Every bug fix MUST include a regression test that fails without the fix.
- Tests MUST NOT be deleted, skipped, or weakened to make a build pass. A disabled test MUST
  reference a tracked `bd` issue.

**Rationale**: Tests written first prove the requirement is testable and capture intent before
implementation details bias the design.

### III. Layered, Meaningful Test Coverage

- Unit tests MUST cover domain logic in isolation and MUST be fast (the unit suite SHOULD run in
  under 60 seconds) and deterministic (no real network, clock, or randomness without seeding).
- Integration tests MUST cover boundaries: persistence, external services, inter-module
  contracts, and public API/CLI surfaces.
- Each user story in a spec MUST have at least one acceptance-level test that exercises it end to
  end.
- Line coverage on changed code MUST be at least 80%. Coverage is a floor, not a goal: tests MUST
  assert observable behavior, not implementation details.
- Flaky tests MUST be fixed or quarantined with a tracked issue within one working day of
  detection.

**Rationale**: Each layer catches a different failure class; determinism keeps the suite
trustworthy enough that a red build always means a real problem.

### IV. Maintainability Through Simplicity

- Build the simplest design that satisfies current, specified requirements (YAGNI).
  Speculative abstractions, plugin points, and configuration knobs MUST NOT be added without a
  current requirement.
- Any added complexity (new dependency, new layer, new pattern) MUST be justified in the plan's
  Complexity Tracking table with the simpler alternative that was rejected and why.
- Modules MUST have a single, clear responsibility and MUST depend on abstractions at boundaries
  so they can be tested in isolation.
- Duplication MAY be tolerated until the third occurrence; then it MUST be extracted.
- New third-party dependencies MUST be actively maintained, license-compatible, and pinned via a
  lockfile.
- Functions exceeding ~50 lines or cyclomatic complexity of 10 MUST be refactored or justified in
  review.

**Rationale**: Code is read and changed far more often than written; every unneeded
abstraction is a permanent maintenance cost.

### V. Explicit Contracts and Documentation

- Public interfaces (APIs, CLIs, exported modules, data schemas) MUST have documented contracts:
  inputs, outputs, error cases.
- Breaking changes to a public contract MUST follow semantic versioning and MUST include a
  migration note.
- Comments MUST explain *why*, not *what*. Non-obvious decisions MUST be recorded as an ADR or in
  the feature's `plan.md`.
- README and build/test instructions MUST stay accurate; a change that alters how the project is
  built, run, or tested MUST update them in the same change.

**Rationale**: Explicit contracts make change safe; recorded reasoning prevents future
maintainers from undoing deliberate decisions.

## Quality Standards

- **Tooling**: The project MUST define single commands for format, lint, type-check, test, and
  coverage, documented in `CLAUDE.md` / `AGENTS.md` under "Build & Test".
  TODO(TECH_STACK): concrete tools to be selected in the first feature plan.
- **Reproducible environment**: The development environment MUST be reproducible from the
  repository (currently `shell.nix` + `.envrc`). All required tools MUST be declared there.
- **Automation**: Every quality gate in this constitution MUST be runnable locally with one
  command and SHOULD run in CI once CI exists.
- **Performance**: Features with performance-sensitive paths MUST state measurable targets in
  their spec and MUST include a test or benchmark that verifies them.

## Development Workflow & Quality Gates

- **Spec-driven flow**: Non-trivial features MUST follow specify → plan → tasks → implement.
  The plan MUST pass the Constitution Check before implementation begins and again after
  design.
- **Task tracking**: All work MUST be tracked in `bd` (beads). Follow-up work discovered during a
  change MUST be filed as a `bd` issue, not left as a TODO comment.
- **Pre-merge gates** (all MUST pass):
  1. Formatter and linter clean.
  2. Type check clean.
  3. Full test suite green, including new tests written first.
  4. Coverage on changed code ≥ 80%.
  5. Documentation updated for any contract or workflow change.
- **Review**: Every change MUST be reviewed (human or agent-assisted review plus human approval)
  against these principles before merge. Reviewers MUST reject changes that violate a MUST
  without a recorded justification.
- **Commits**: Commits MUST be small, focused, and use Conventional Commits. Agents MUST NOT
  commit or push without explicit authorization.

## Governance

- This constitution supersedes all other development practices. Where guidance files
  (`CLAUDE.md`, `AGENTS.md`) conflict with it, the constitution wins and the guidance file MUST be
  corrected.
- **Amendments** MUST be made via `/speckit-constitution`, documented with a Sync Impact Report,
  reviewed, and approved by the project maintainer. Amendments that tighten rules MUST include a
  migration plan for existing code.
- **Versioning** follows semantic versioning:
  - MAJOR: removal or backward-incompatible redefinition of a principle or governance rule.
  - MINOR: new principle or section, or materially expanded guidance.
  - PATCH: clarifications, wording, and typo fixes.
- **Compliance review**: Every plan's Constitution Check and every code review MUST verify
  compliance. Deviations MUST be recorded in the plan's Complexity Tracking table with
  justification. The constitution SHOULD be reviewed for relevance at least once per quarter.
- Runtime development guidance lives in `CLAUDE.md` and `AGENTS.md`.

**Version**: 1.0.0 | **Ratified**: 2026-09-17 | **Last Amended**: 2026-09-17
