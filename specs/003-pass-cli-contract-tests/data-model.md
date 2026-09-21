# Phase 1 Data Model: Real `pass-cli` contract and live tests

This feature adds no production types. The entities below are test-harness types that live in
`tests/support/real_cli.rs` and are used by both new test binaries. They are listed here because
the harness is the part of this feature with real structure; the tests themselves are flat.

---

## `RealCli`

The concrete binary a run resolved, plus everything a probe needs to drive it.

| Field | Type | Notes |
|---|---|---|
| `path` | `PathBuf` | From `COSMIC_PASS_CLI`, else `pass-cli` resolved on `PATH`. |
| `version` | `CliVersion` | Parsed from `--version` with `pass::parse::parse_version`. Read once per run. |

**Construction**: `RealCli::find() -> Resolution` (see below). Resolution happens once per test
binary through a `std::sync::OnceLock`, so the version banner is parsed once rather than per test.

**Validation rules**:

- `version >= core::version::TESTED_MIN`, else the run fails naming both versions (FR-005, D5).
- A version above `TESTED_MIN` is valid and is printed, not asserted on (D5).

**Invariants**:

- Every probe in one test binary uses the same `path`. Resolving per test would allow a suite to
  straddle two binaries and report a meaningless result.

---

## `Resolution`

What discovery produced. Models the skip-versus-fail rule as data rather than as control flow
scattered through each test.

| Variant | Meaning | Test behaviour |
|---|---|---|
| `Found(RealCli)` | A binary resolved and its version is in range. | Run the probe. |
| `Skip { looked_for: String }` | Nothing resolved and `COSMIC_PASS_REQUIRE_CLI` is unset. | Print `SKIP <binary>: <reason>` to stderr and return. |
| `Fail { reason: String }` | Nothing resolved while `COSMIC_PASS_REQUIRE_CLI=1`, or the version is below `TESTED_MIN`, or `--version` did not parse. | `panic!` with the reason. |

**State transitions**: none. `Resolution` is computed once and never changes within a run.

---

## `IsolatedEnv`

A throwaway home in which a contract probe runs, so the developer's real session is untouched
(FR-012, D3).

| Field | Type | Notes |
|---|---|---|
| `dir` | `TempDir` | Owns the lifetime; removed on drop. |
| `vars` | `Vec<(String, String)>` | `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` all under `dir`. |

**Validation rules**:

- `DBUS_SESSION_BUS_ADDRESS` MUST be cleared for the child, and `PROTON_PASS_LINUX_KEYRING` MUST
  NOT be `dbus`, so no probe can reach the Secret Service (D3).
- After any probe, nothing outside `dir` may have been written. Measured: 2.3.3 writes exactly
  `dir/share/proton-pass-cli/.session/pass-cli.db`.
- A probe that reaches an authenticated state is a failed isolation and fails the test. This is
  the positive assertion that proves the rules above held.

**Relationships**: one `IsolatedEnv` per contract test; it is layered onto `RealCli` by
`TokioRunner::with_env`, so the production runner code path is the one under test (D10).

---

## `ContractProbe`

One assertion, named by the clause of the consumed-interface contract it backs. Not a runtime
struct — it is the shape every contract test follows, and the table that
`contracts/pass-cli-test-harness.md` keeps in sync.

| Attribute | Meaning |
|---|---|
| `clause` | The contract line this probe defends, e.g. "`item list --share-id=... --output json --show-secrets`". |
| `invocation` | The argv sent to the real binary. |
| `expectation` | Exit status, plus the tokens required in stdout, or the `PassError` variant `classify` must produce from stderr. |

**Invariant**: every clause of `specs/001-quick-access-launcher/contracts/pass-cli.md` is either
covered by a `ContractProbe`, covered by a `LiveScenario`, or listed as manual-only in the
harness contract with a reason (SC-001).

---

## `LiveScenario`

One read-only end-to-end exercise against a signed-in account.

| Attribute | Meaning |
|---|---|
| `name` | Stable identifier printed in the coverage report, e.g. `field-in-section`. |
| `prerequisite` | The item kind the account must hold, e.g. "an item with a TOTP field". |
| `parser` | The `pass::parse` function whose success is the assertion. |
| `outcome` | `Covered`, or `NotCovered { reason }` when the prerequisite is absent (FR-017). |

**State transitions**: `Pending -> Covered` or `Pending -> NotCovered`. A scenario left `Pending`
at the end of a run is a harness bug and fails the run; it must not silently disappear.

**Invariant**: no `LiveScenario` may mutate the account. Only `info`, `vault list`, `item list`,
`item view` and `item totp` are invoked (FR-016).

---

## `FixtureShape`

The comparable skeleton of a JSON fixture (FR-019, D9).

| Field | Type | Notes |
|---|---|---|
| `paths` | `BTreeSet<(String, ValueKind)>` | Dotted key path, with array indices collapsed to `[]`, paired with the kind at that path. |

`ValueKind` is `Null | Bool | Number | String | Array | Object`.

**Derivation**: walk a `serde_json::Value`, emitting one entry per leaf and per container.
`{"vaults":[{"share_id":"s"}]}` yields `("vaults", Array)`, `("vaults[]", Object)`,
`("vaults[].share_id", String)`.

**Validation rules**:

- Comparison is on `paths` only. No value is ever compared, printed, or written (FR-020).
- A difference is reported as two sorted lists — paths only in fresh output, and paths only in
  the committed fixture — so the report names what upstream added or removed.

**Relationships**: one `FixtureShape` per committed file under
`tests/fixtures/pass-cli/captured/`, compared against one derived from fresh redacted output for
the corresponding command.
