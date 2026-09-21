# Contract: the real-`pass-cli` test harness

This is the contract between a developer (or the dev shell) and the two new test binaries. The
*consumed* `pass-cli` interface itself is not restated here; it lives in
`specs/001-quick-access-launcher/contracts/pass-cli.md`, and the coverage table at the bottom of
this file says which of its clauses are now checked automatically.

## Test binaries

| Binary | Runs in | Needs an account | Command |
|---|---|---|---|
| `tests/pass_cli_contract.rs` | `just test`, `just check` | no | `cargo nextest run` |
| `tests/pass_cli_live.rs` | manual only | yes | `just test-live` |

## Environment contract

| Variable | Read by | Meaning |
|---|---|---|
| `COSMIC_PASS_CLI` | both | Path to the `pass-cli` under test. Unset: `pass-cli` from `PATH`. Same variable `TokioRunner::from_env` uses, so the app and its tests never disagree about which binary is under test. |
| `COSMIC_PASS_REQUIRE_CLI` | contract | `1` turns "no `pass-cli` found" from a skip into a failure. Set by the flake devShell. |
| `COSMIC_PASS_LIVE` | live | `1` permits the live suite to run. Without it, every live test returns immediately even under `--run-ignored=only`. |

No other variable changes behaviour. `PASS_LOG_LEVEL`, `PROTON_PASS_NO_UPDATE_CHECK` and
`PROTON_PASS_LINUX_KEYRING` are set *by* the harness (through `TokioRunner`), not read from the
developer's environment.

## Exit behaviour

| Situation | Contract suite | Live suite |
|---|---|---|
| Binary found, version in range | runs | runs |
| Binary found, version < `TESTED_MIN` | **fails**, naming found and required versions | fails, same |
| Binary found, version > `TESTED_MIN` | runs; prints the version seen | runs; prints the version seen |
| No binary, `COSMIC_PASS_REQUIRE_CLI` unset | prints `SKIP pass_cli_contract: no pass-cli on PATH and COSMIC_PASS_CLI unset`, reports pass | same shape |
| No binary, `COSMIC_PASS_REQUIRE_CLI=1` | **fails** | fails |
| `COSMIC_PASS_LIVE` unset | n/a | returns immediately, prints `SKIP pass_cli_live: set COSMIC_PASS_LIVE=1` |
| `COSMIC_PASS_LIVE=1`, no session | n/a | **fails** with `not signed in; run: pass-cli login` |

Skip notices go to stderr, always as `SKIP <binary>: <reason>`, so they are greppable and visible
in `just check` output.

## Isolation contract (contract suite only)

Every contract probe runs with:

- `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` inside a
  per-test temporary directory;
- `DBUS_SESSION_BUS_ADDRESS` cleared, and `PROTON_PASS_LINUX_KEYRING` not set to `dbus`, so no
  probe can reach the Secret Service where a real session key lives.

Guarantees, each asserted rather than assumed:

1. No probe reaches an authenticated state. A probe that does means isolation failed; the test
   fails and says so.
2. Nothing outside the temporary directory is written. Measured on 2.3.3: the only file created
   is `$XDG_DATA_HOME/proton-pass-cli/.session/pass-cli.db`.
3. The developer's `pass-cli` session survives a full contract run unchanged.

## Safety contract (live suite only)

1. Read-only. Only `info`, `vault list`, `item list`, `item view` and `item totp` are invoked.
   No `login`, `logout`, `item create`, `item edit`, `item delete`, `vault` mutation, or
   `settings` write.
2. No secret value is formatted into an assertion message, printed, or written to disk.
   Assertions are on parser success, collection non-emptiness, value length ranges, and
   character classes.
3. No secret value is passed in argv.
4. Each scenario reports `covered` or `not covered: <reason>`; the run prints the full table at
   the end so a maintainer can see what the account could not exercise.

## Coverage of the consumed interface

Which clauses of `specs/001-quick-access-launcher/contracts/pass-cli.md` this feature checks.
This table is the mechanism behind SC-001 and must be kept in sync when a clause is added there.

| Clause | Checked by | How |
|---|---|---|
| Binary is `$COSMIC_PASS_CLI` or `pass-cli` on `PATH` | contract | discovery is the harness itself |
| Env added: `PASS_LOG_LEVEL=off`, `PROTON_PASS_NO_UPDATE_CHECK=1` | contract | accepted without error; **stdout** carries payload only |
| Env added: `PROTON_PASS_LINUX_KEYRING=dbus` | live | the live suite runs with the production environment |
| stdin null; stdout/stderr piped | already covered | `tests/pass_cli_integration.rs`, fake CLI |
| Own process group, killed on drop/timeout/cancel | already covered | `tests/pass_cli_integration.rs`, fake CLI |
| Per-command timeouts | already covered | `tests/pass_cli_integration.rs`, fake CLI |
| At most 4 processes at once | already covered | `tests/pass_cli_integration.rs`, fake CLI |
| Secrets never in argv | contract + live | argv assertions in both suites |
| IDs must be `--flag=VALUE`; space form fails | contract | `item list --share-id -leadingdash` yields `unexpected argument` |
| `info --output json` shape | live | `parse_account` |
| `vault list --output json` shape | live | `parse_vaults` |
| `item list --output json` shape (plain) | live | `parse_items` |
| `item list --output json --show-secrets` shape | live | `parse_items` |
| `item view --field=<F>` returns a raw non-JSON value | live | `parse_field` |
| `item view --field=<Section>.<Name>` | live | the 2.1.4 regression scenario |
| Missing field → `Field does not exist` | live | classifies as `FieldMissing` |
| `item totp --output json` shape | live | `parse_totp`, six-digit check |
| `pass-cli login` behaviour | **manual** | interactive web flow; not automatable (quickstart V5) |
| `--version` banner | contract | `parse_version`, floor check |
| Command and flag surface for every command the app sends | contract | `--help` token assertions |
| `SignedOut` mapping | contract | real unauthenticated stderr through `classify` |
| `NotFound` mapping | live | malformed share id and missing item, with a session present |
| `FieldMissing` mapping | live | missing field on a real item |
| `Locked` mapping | **manual** | text never observed; noted as unverified in the consumed contract |
| `Network` mapping | **manual** | requires induced network failure; out of scope |
| `LocalData` mapping | **manual** | requires a corrupted local store; out of scope |
| `Protocol` mapping | already covered | `src/pass/parse.rs` unit tests |
| Latency figures | reported, not asserted | printed by the live suite |
| Captured fixtures resemble real output | live | `FixtureShape` set comparison |

## Amendments this feature makes to the consumed contract

Both are corrections to `specs/001-quick-access-launcher/contracts/pass-cli.md`, to be applied in
the same change:

1. **`PASS_LOG_LEVEL=off` does not silence error-level logging.** 2.3.3 still writes a coloured
   `tracing` `ERROR` line to stderr before the `Error:` line. The contract currently says a
   coloured tracing line may come first "with `PASS_LOG_LEVEL` unset"; it comes first regardless.
   Harmless — `classify` substring-matches and `summary_line` prefers the `Error:` line — but the
   contract must not imply stderr is quiet.
2. **The error-mapping table is missing the `LocalData` rule.** `src/pass/error.rs` matches
   `file is not a database`, `logout --force` and `failed to initialize database` to
   `LocalData`, ahead of the `NotFound` and `Network` rules, and also matches `forcing logout`
   into `SignedOut`. Neither appears in the table. The table is the documented order the tests
   assert against, so it must match the code.
