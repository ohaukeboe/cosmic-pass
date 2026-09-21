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

A probe's home is **stable, not random**. `IsolatedEnv::new(label)` puts it at
`target/pass-cli-probe-homes/<label>` and wipes it; only `IsolatedEnv::stateless()`, for probes
that run just `--help` or `--version`, uses a `TempDir`. The reason is the keyring, not the
filesystem: `pass-cli` encrypts its store with a key it keeps in the **persistent** kernel
keyring under `keyring:cli-local-key:<sha256 of the store path>@ProtonPassCLI`, marked `perm`.
That keyring is per-uid, so no `HOME` override reaches it, and `logout --force` does not clear
it. With a random path per run the suite minted six permanent keys every run against a 200-key,
20000-byte quota shared with everything else the developer runs; it filled, and every probe then
failed with `Error accessing keyring: Platform failure: QuotaExceeded`. A stable path makes the
description repeat, so each probe's key is created once and reused for ever after.

Every contract probe runs with:

- `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` inside a
  per-test temporary directory;
- `PROTON_PASS_LINUX_KEYRING=kernel` (upstream's own default) instead of the production
  `dbus`, and `DBUS_SESSION_BUS_ADDRESS` pointed at `unix:path=/nonexistent/cosmic-pass-test`,
  so no probe can reach the Secret Service where a real session key lives. Pointed at nothing
  rather than unset, because `TokioRunner` can add variables for a child but not remove them,
  and growing a production API for one test would cost more than it buys.

Guarantees, each asserted rather than assumed:

1. No probe reaches an authenticated state. A probe that does means isolation failed; the test
   fails and says so.
2. Everything written lands inside the probe's own home. Measured on 2.3.3: the only file
   created is `$XDG_DATA_HOME/proton-pass-cli/.session/pass-cli.db`. A probe that wrote
   somewhere else would have ignored the overrides, which is what the assertion catches. The
   one thing a probe leaves outside its home is its kernel-keyring key, which guarantee 4
   bounds.
3. The developer's `pass-cli` session survives a full contract run unchanged.
4. A probe's kernel-keyring key is created once and reused, never accumulated. Asserted by
   `keyring::a_probe_reuses_one_kernel_key`, which reproduces the description from the store
   path and checks that a second `IsolatedEnv` with the same label lands on the same path and
   the same description; and by `keyring::help_and_version_touch_no_store`, which is why
   `stateless()` is allowed a random path. Repeated whole-suite runs were measured to leave the
   key count unchanged.
5. No probe outlives its deadline. Probes that go through `TokioRunner` carry the app's own
   timeout; the raw probes -- the ones that need the exit status and stderr the runner hides --
   go through `RawOutput::try_capture`, which kills the child once `PROBE_TIMEOUT` (10 s) has
   passed and reports the timeout as a failure. Version resolution takes the same path, so a
   `pass-cli` that hangs on `--version` fails resolution rather than wedging the binary before
   a test has started.

## Safety contract (live suite only)

1. Read-only. Only `info`, `vault list`, `item list`, `item view` and `item totp` are invoked.
   No `login`, `logout`, `item create`, `item edit`, `item delete`, `vault` mutation, or
   `settings` write.
2. No secret value is formatted into an assertion message, printed, or written to disk.
   Assertions are on parser success, collection non-emptiness, value length ranges, and
   character classes.
3. No secret value is passed in argv.
4. Each scenario prints `SCENARIO <name>: covered` or `SCENARIO <name>: not covered: <reason>`.
   One line per scenario rather than one table at the end: nextest runs each test in its own
   process, so there is nowhere for a shared table to live. `just test-live` passes
   `--no-capture` so the lines reach the terminal.

## Coverage of the consumed interface

Which clauses of `specs/001-quick-access-launcher/contracts/pass-cli.md` this feature checks.
This table is the mechanism behind SC-001 and must be kept in sync when a clause is added there.

| Clause | Checked by | Test |
|---|---|---|
| Binary is `$COSMIC_PASS_CLI` or `pass-cli` on `PATH` | contract | `support::real_cli::resolve` -- discovery is the harness itself |
| Env added: `PASS_LOG_LEVEL=off`, `PROTON_PASS_NO_UPDATE_CHECK=1` | contract | `env::stdout_carries_payload_only` (stdout only; stderr is never quiet -- see amendment 1) |
| Env added: `PROTON_PASS_LINUX_KEYRING=dbus` | live | every live test; the suite runs with the production environment |
| stdin null; stdout/stderr piped | already covered | `pass_cli_integration.rs::runner::sets_quiet_env_and_null_stdin` |
| Own process group, killed on drop/timeout/cancel | already covered | `pass_cli_integration.rs::runner::{timeout,cancel,dropping}_kills_the_process` |
| Per-command timeouts | already covered | `pass_cli_integration.rs::runner` |
| A probe's kernel-keyring key is created once per probe home and reused, never accumulated | contract | `keyring::a_probe_reuses_one_kernel_key`, `keyring::help_and_version_touch_no_store` |
| A probe that hangs is killed, not waited on | contract | `timeout::a_hung_probe_is_killed_rather_than_waited_on` -- a stand-in that outlives its deadline on purpose; the real binary cannot be made to hang on demand |
| At most 4 processes at once | already covered | `pass_cli_integration.rs::runner::runs_at_most_four_processes_at_once` |
| Secrets never in argv | contract + live | argv comes from `backend::argv`, which takes ids and field names only |
| IDs must be `--flag=VALUE`; space form fails | contract | `argv::ids_must_use_the_equals_form` |
| `info --output json` shape | live | `info_parses` (`parse_account`) |
| `vault list --output json` shape | live | `vault_list_parses` (`parse_vaults`) |
| `item list --output json` shape (plain) | live | `item_list_parses_plain` (`parse_items`) |
| `item list --output json --show-secrets` shape | live | `item_list_parses_with_secrets` (`parse_items`) |
| `item view --field=<F>` returns a raw non-JSON value | live | `field_view_parses` (`parse_field`) |
| `item view --field=<Section>.<Name>` | live | `field_inside_a_section_parses` -- the 2.1.4 regression |
| Missing field -> `Field does not exist` | live | `missing_field_is_field_missing` |
| `item totp --output json` shape | live | `totp_parses` (`parse_totp`, six-digit check) |
| `pass-cli login` behaviour | **manual** | interactive web flow; not automatable (feature 001 quickstart V5). Its argv is still probed: `surface::top_level_commands_exist` |
| `--version` banner | contract | `version::reported_version_is_at_or_above_tested_min`, `argv::the_version_command_the_app_builds_answers` |
| Command and flag surface for every command the app sends | contract | `surface::*` (six probes) and `argv::every_command_the_app_builds_is_accepted` |
| `SignedOut` mapping | contract | `classify::unauthenticated_stderr_is_signed_out` |
| `NotFound` mapping | live | `malformed_share_id_is_not_found` |
| `FieldMissing` mapping | live | `missing_field_is_field_missing` |
| `Locked` mapping | **manual** | text never observed upstream; still marked unverified in the consumed contract |
| `Network` mapping | **manual** | needs an induced network failure; out of scope |
| `LocalData` mapping | **manual** | needs a corrupted local store; out of scope |
| `Protocol` mapping | already covered | `src/pass/parse.rs` unit tests |
| Latency figures | reported, not asserted | `latency_is_reported` |
| Captured fixtures resemble real output | live | `committed_fixtures_still_match_reality` |

Measured on 2026-09-21 against 2.3.3: the contract suite is 23 tests in 2.4 s -- all of it the
deliberate 2 s sleep in `timeout::a_hung_probe_is_killed_rather_than_waited_on`, the other twenty-two
finishing in about 1 s; the live suite is 11 tests in about 57 s and reported every scenario
`covered`. The `fixture-shape-share-2` scenario postdates that run and has not yet been observed
against an unlocked session.

**Known limit of the fixture-shape check.** `scripts/capture-fixtures.sh` keeps a sample of
items, so which optional sub-objects a fixture carries depends on which items were sampled. A
key path present in fresh output but missing from the fixture is therefore usually sampling
chance, not upstream drift -- which is why an added path is only reported, and only a *removed*
path fails.

**Per-vault fixtures.** `item-list-share-1.json` and `item-list-plain-share-1.json` are compared
against the first vault in `vault list`, the `-share-2` pair against the second. Both fixture and
fresh output are per-vault, so pairing them any other way would compare two unrelated samples. An
account with a single vault therefore leaves the `-share-2` pair unchecked; that is reported as
`SCENARIO fixture-shape-share-2: not covered: ...` rather than passing silently (FR-017). The 2026-09-21 run reported 31 added paths against `item-list-share-1.json`
(`Login.passkeys[]`, `content.platform_specific`, `extra_fields[].content.Totp`,
`Custom.sections[].section_fields[].content.Text`) and no removals.

## Amendments this feature makes to the consumed contract

Both are corrections to `specs/001-quick-access-launcher/contracts/pass-cli.md`, applied in this
change:

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
