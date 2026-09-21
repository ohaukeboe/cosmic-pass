# Phase 0 Research: Real `pass-cli` contract and live tests

All probes below were run on 2026-09-21 against `pass-cli` 2.3.3 (`0d7235d`), the version the
`nixpkgs-pass-cli` flake input pins (`/nix/store/yrypmar7kxxswbrj1dc9lmrjgz18m1s4-proton-pass-cli-2.3.3`)
and the same one already on the developer's `PATH`. Every probe ran with a throwaway `HOME`
and XDG directories so the developer's real session was never touched.

---

## D1. Where the tests live and how they are shaped

**Decision**: Two new integration test binaries under `tests/`:
`tests/pass_cli_contract.rs` (default suite, no account) and `tests/pass_cli_live.rs`
(opt-in, signed-in account), sharing a helper module `tests/support/real_cli.rs`.

**Rationale**: The repo already keeps boundary tests as `tests/*.rs` integration binaries
(`pass_cli_integration.rs`, `cache_integration.rs`, the four story tests). Two binaries keep the
default gate and the manual gate separable by nextest filter (`-E 'binary(pass_cli_live)'`)
without a cargo feature. A shared module avoids a third copy of binary discovery.

**Alternatives considered**:

- *Extend `tests/pass_cli_integration.rs`*: rejected. That file is the fake-CLI suite; mixing a
  real binary into it makes "which CLI is this test driving?" ambiguous at a glance.
- *A cargo feature `live-tests`*: rejected. Features change what compiles; these tests always
  compile and differ only in whether they run. `#[ignore]` plus an env gate is the smaller tool.
- *A separate crate in a workspace*: rejected under Principle IV; nothing here needs its own
  dependency set.

---

## D2. Binary discovery and the skip-versus-fail rule

**Decision**: Resolve in this order — `COSMIC_PASS_CLI` (the same variable
`TokioRunner::from_env` uses, `src/pass/runner.rs:107`), else `pass-cli` on `PATH`. When nothing
resolves: fail if `COSMIC_PASS_REQUIRE_CLI=1`, otherwise print a one-line skip notice and return
from the test.

**Rationale**: Reusing `COSMIC_PASS_CLI` means a developer pointing the app at a particular
binary automatically points the tests at the same one. The require-flag is set by the flake
devShell, so the constitution's "no skipped tests" rule holds everywhere the project controls
the environment, while a contributor running bare `cargo test` outside Nix still gets green.

**Alternatives considered**:

- *Always fail when absent*: rejected. `cargo test` on a non-Nix machine would be permanently
  red, which trains people to ignore red.
- *Always skip when absent*: rejected. A broken flake input would then silently disable the
  entire feature — exactly the failure this feature exists to prevent.
- *`#[ignore]` on the contract suite too*: rejected. It would never run in `just check`.

**Note on mechanics**: cargo/nextest have no runtime "skipped" verdict; a dynamic skip is a test
that prints and returns, and therefore reports as passed. The notice line is written to stderr in
a fixed `SKIP pass_cli_contract: ...` form so it is greppable and so `just check` output shows it.

---

## D3. Isolating a probe from the developer's real session

**Decision**: Each contract probe runs with `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
`XDG_STATE_HOME` and `XDG_CACHE_HOME` pointed at a per-test `tempfile::TempDir`, with
`DBUS_SESSION_BUS_ADDRESS` removed and `PROTON_PASS_LINUX_KEYRING` **not** set to `dbus`.
The suite asserts positively that the probe is unauthenticated, which is what proves the
isolation held.

**Rationale**: Measured — a probe under an overridden `XDG_DATA_HOME` created exactly one file,
`$XDG_DATA_HOME/proton-pass-cli/.session/pass-cli.db`, and nothing outside the temp directory.
So overriding the XDG data dir is sufficient for the on-disk store.

The keyring is the dangerous half. Production sets `PROTON_PASS_LINUX_KEYRING=dbus`
(`src/pass/runner.rs:24`), which puts the session key in the Secret Service — a bus, not a
directory, so a temp `HOME` does not isolate it. A probe that inherited `dbus` could read or
disturb the developer's real session key. Dropping the bus address from the probe environment
removes the reach.

The cost is that the probes do not exercise the `dbus` value itself. That value only decides
where a *login* stores its key, and login cannot be exercised without an account in the first
place, so nothing testable is lost from the contract suite; the live suite runs with the real
environment and therefore does cover it.

**Alternatives considered**:

- *Inherit the real environment and only override `HOME`*: rejected on safety. The suite would
  hold a real authenticated session, and a stray assertion could print a real secret.
- *A container or bwrap sandbox per probe*: rejected under Principle IV. A temp directory plus a
  pruned environment is measurably enough, and the suite asserts the outcome rather than trusting
  the mechanism.

---

## D4. What the contract suite can actually assert without an account

Probe results against 2.3.3, with the consumed-interface contract clause each one backs:

| Probe | Result | Backs |
|---|---|---|
| `--version` | `Proton Pass CLI 2.3.3 (0d7235d)`, exit 0 | version banner, `parse_version` |
| `--help` | lists `login`, `logout`, `info`, `vault`, `item`, exit 0 | top-level commands |
| `info --help` | `-o, --output <OUTPUT> [possible values: human, json]` | `info --output json` |
| `vault list --help` | `--output <OUTPUT> [possible values: human, json]` | `vault list --output json` |
| `item list --help` | `--share-id`, `--output`, `--show-secrets` all present | `item list` argv |
| `item view --help` | `--share-id`, `--item-id`, `--field`, `--output` all present | `item view` argv |
| `item totp --help` | `--share-id`, `--item-id`, `--output` all present | `item totp` argv |
| `info --output json`, unauthenticated | exit 1, stderr `Error: This operation requires an authenticated client` | `SignedOut` |
| `vault list --output json`, unauthenticated | exit 1, same stderr | `SignedOut` |
| `item list --share-id -leadingdash --output json` | exit 2, stderr `error: unexpected argument '-l' found` | the `--flag=VALUE` rule |
| `item list --share-id=bogus --output json`, unauthenticated | exit 1, **`SignedOut`, not `NotFound`** | see below |

**Finding — the malformed-share-id case is not reachable offline.** Spec FR-010 assumed
`--share-id=bogus` would produce the `idformat` error captured in
`tests/fixtures/pass-cli/captured/errors.txt`. It does not: 2.3.3 checks authentication first and
returns `This operation requires an authenticated client`. The `NotFound` classification rule
therefore has no offline probe and moves to the live suite. This is recorded rather than worked
around; inventing a way to reach it offline would be testing a path the app never takes.

**Finding — `PASS_LOG_LEVEL=off` does not silence error-level logging.** An unauthenticated run
still emits a `tracing` line to stderr:

```
2026-09-21T10:23:13.289460Z ERROR pass-cli/src/main.rs:332: Command is not logout there is no session
Error: This operation requires an authenticated client
```

with ANSI escapes. This is harmless for the app — `classify` lowercases and substring-matches, so
both lines route to `SignedOut`, and `summary_line` picks the `Error:`-prefixed line — but the
contract's "env added: `PASS_LOG_LEVEL=off`" clause must not be read as "stderr is quiet". The
contract suite asserts only what the app depends on: that **stdout** carries nothing but the
payload, and that stderr classifies correctly. Recorded as a contract amendment in
`contracts/pass-cli-test-harness.md`.

**Decision**: Assert the help surface by parsing `--help` output for the exact flag tokens the
app sends, rather than by running the commands and inspecting failures.

**Rationale**: `--help` needs no session, exits 0, and distinguishes "the flag was removed" from
"the flag exists but you are signed out" — the two outcomes a failure-text probe would conflate.

**Alternatives considered**:

- *Invoke each command for real and assert the error is `SignedOut` rather than a clap usage
  error*: kept as a complement for `item list`/`info`/`vault list`, since it proves the whole argv
  the app builds is accepted, not just that the tokens appear in help text. Both are cheap.
- *Snapshot the full `--help` text with `insta`*: rejected. Upstream rewords help prose freely;
  a snapshot would fail on every reword and teach people to accept blindly. Assert the tokens the
  app depends on, nothing more.

---

## D5. Version floor handling

**Decision**: The contract suite fails when the resolved binary reports a version below
`core::version::TESTED_MIN` (currently 2.3.0), and prints the version it saw otherwise.

**Rationale**: Below the floor, `item view --field` cannot address a field inside a section
(`src/core/version.rs:19`), so the contract suite would be asserting against a binary the app
itself warns about. Above the floor is deliberately not an error — upstream ships roughly weekly,
and `warning()` is silent for newer versions by design. The printed line is the signal a
maintainer uses to decide to raise the floor.

**Alternatives considered**:

- *Pin the test to an exact version*: rejected. It would fail on every user's own newer
  `pass-cli`, which is precisely the configuration the `--suffix PATH` packaging allows.

---

## D6. Delivering the pinned binary to the dev shell

**Decision**: Add `nixpkgs-pass-cli.legacyPackages.${system}.proton-pass-cli` to the devShell
`packages`, and set `COSMIC_PASS_REQUIRE_CLI = "1"` in the same shell.

**Rationale**: The input and its pin already exist for the package output
(`flake.nix:19`, `flake.nix:135`); reusing it costs one line and guarantees the tests run against
the exact binary the packaged app wraps. The require-flag turns "the flake stopped providing
`pass-cli`" from a silent skip into a failure.

**Alternatives considered**:

- *`pkgs.proton-pass-cli` from the main nixpkgs*: rejected. `flake.nix:13` records that
  nixos-26.05 ships 2.0.2 and nixos-25.11 does not package it at all; the dev shell must test the
  version the package ships.
- *A separate `nix develop .#live` shell*: rejected. Nothing differs between the two shells but
  an env var the developer can set inline.

---

## D7. Running the live suite

**Decision**: Every live test is `#[ignore = "..."]` **and** gated on `COSMIC_PASS_LIVE=1`; a new
`just test-live` recipe runs `cargo nextest run --run-ignored=only -E 'binary(pass_cli_live)'`
with that variable set. A preflight test asserts a session exists and stops with a message naming
`pass-cli login` when it does not.

**Rationale**: `#[ignore]` keeps it out of `just test`. The extra env gate keeps a developer who
runs `cargo test -- --ignored` for unrelated reasons from unexpectedly driving their real vault.

**Alternatives considered**:

- *`#[ignore]` alone*: rejected — `--ignored` is a blunt, commonly-used flag.
- *Env gate alone*: rejected — leaves the tests in the default suite, where a stray exported
  variable would put real-account traffic inside `just check`.

---

## D8. Keeping secrets out of live test output

**Decision**: Live assertions are on shape only — parser success, collection non-emptiness,
`SecretString` length ranges, and "the TOTP code is six digits" via a character-class check.
Nothing obtained from a real account is formatted into an assertion message, and `scripts/leak-scan.sh`
is extended to cover the live run's output.

**Rationale**: `just test` failure output is routinely pasted into issues and chats, and
`ExposeSecret` in a failure message would land there. The existing `leak-scan` recipe already
exists for exactly this class of mistake; reusing it is free.

**Alternatives considered**:

- *Compare against expected values from a fixture account*: rejected. It requires provisioning
  test data the feature explicitly does not own, and puts known secrets in the repo.

---

## D9. Fixture shape drift

**Decision**: A live test re-runs the redaction logic over fresh output, reduces both fresh and
committed fixtures to a sorted set of `key.path -> value kind` entries, and asserts set equality,
reporting added and removed paths separately.

**Rationale**: `src/pass/parse.rs:1361` already proves the committed fixtures parse; nothing
proves they still resemble what upstream emits. Key paths and value kinds are exactly the surface
`serde` cares about and carry no secret material. Comparing values would be meaningless across
accounts.

**Alternatives considered**:

- *Re-run `scripts/capture-fixtures.sh` and `git diff`*: rejected as a test. It rewrites tracked
  files as a side effect, and the ids and redaction placeholders differ per account, so the diff
  is noise. The script stays the way fixtures are refreshed deliberately; the test only reports.
- *A JSON Schema per fixture*: rejected under Principle IV. A second schema language to maintain
  alongside the `serde` structs that already are the schema.

---

## D10. Timeouts and runtime budget

**Decision**: Probes reuse `TokioRunner` with the app's own timeouts where they drive real
commands, and a flat 10 s for `--help`-style probes. The whole contract suite targets well under
the 30 s budget in SC-003.

**Rationale**: Driving the real commands through `TokioRunner` rather than `std::process` means
the test also covers the process-group kill, null stdin and env injection the runner performs —
the same code path production uses. Measured locally, each unauthenticated probe returns in about
10 ms; the eleven probes in D4 total well under a second.

**Alternatives considered**:

- *Bypass `TokioRunner` and call `std::process::Command` directly*: rejected. It would test a
  command line the app never actually builds.
