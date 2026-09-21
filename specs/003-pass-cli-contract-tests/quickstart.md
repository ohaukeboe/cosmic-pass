# Quickstart: validating the real-`pass-cli` tests

How to run the two new suites and how to prove they do what they claim. Details of what is
asserted live in [contracts/pass-cli-test-harness.md](./contracts/pass-cli-test-harness.md);
this file is the run guide.

## Prerequisites

- The dev shell: `nix develop`, or `direnv allow`. It provides the pinned `proton-pass-cli`
  (2.3.3) and sets `COSMIC_PASS_REQUIRE_CLI=1`.
- For the live suite only: a signed-in `pass-cli` (`pass-cli login`) on an account holding at
  least one item with a password, one item with a TOTP field, and one item with a field inside a
  named section. Without the last two, those scenarios report *not covered* rather than failing.

## Commands

```bash
just test                 # contract suite runs as part of the default suite
just check                # full pre-merge gate, contract suite included
just test-live            # opt-in live suite; needs a signed-in pass-cli

# Just the contract suite:
cargo nextest run -E 'binary(pass_cli_contract)'

# Point both suites at a different pass-cli:
COSMIC_PASS_CLI=/path/to/pass-cli just test
```

---

## V1. The contract suite runs and passes in the dev shell

```bash
nix develop -c just test
```

**Expect**: green, with no `SKIP pass_cli_contract:` line in the output. A skip line here means
the flake stopped providing `proton-pass-cli`; with `COSMIC_PASS_REQUIRE_CLI=1` set by the shell
it should be a failure, so a skip line is itself a bug.

## V2. A contributor without `pass-cli` still gets green

```bash
env -u COSMIC_PASS_REQUIRE_CLI PATH=/usr/bin:/bin cargo nextest run -E 'binary(pass_cli_contract)'
```

**Expect**: green, with `SKIP pass_cli_contract: no pass-cli on PATH and COSMIC_PASS_CLI unset`
on stderr.

## V3. A missing binary inside the dev shell fails

```bash
COSMIC_PASS_CLI=/nonexistent/pass-cli COSMIC_PASS_REQUIRE_CLI=1 \
  cargo nextest run -E 'binary(pass_cli_contract)'
```

**Expect**: red, naming the path it looked for. This is the guard that stops a broken flake pin
from silently disabling the feature.

## V4. Drift is actually detected — the SC-002 demonstration

Temporarily break one thing in `src/pass/backend.rs`, run the contract suite, then revert:

| Mutation | Expected failure |
|---|---|
| `--show-secrets` → `--show-secret` | the `item list` flag-surface assertion |
| `item totp` → `item otp` | the `item totp` subcommand assertion |
| `format!("--share-id={}", ...)` → a space-separated `--share-id` argument | the `--flag=VALUE` rule assertion |

**Expect**: exactly one assertion fails per mutation, and its message names the contract clause.
Broad collateral failure means the assertions are coupled and should be tightened.

## V5. The developer's real session is untouched

```bash
pass-cli info --output json | head -1     # note the account
nix develop -c just test
pass-cli info --output json | head -1     # same account, still signed in
```

**Expect**: identical output before and after. Also check no stray store was created:

```bash
git status --porcelain            # clean
ls ~/.local/share/proton-pass-cli # unchanged mtime on the session dir
```

## V6. The live suite against a real account

```bash
pass-cli login          # if not already signed in
just test-live
```

**Expect**: green, and a printed coverage table with one row per scenario reading `covered` or
`not covered: <reason>`. Latency figures for `info`, `vault list` and `item list` are printed for
comparison with the numbers recorded in the consumed contract (`info` and `vault list` ~0.6 s;
`item list --show-secrets` over two vaults ~3.6 s).

**Also verify**: nothing in the output is a real secret. A marker grep cannot prove this --
a real account's secrets carry no marker -- so `scripts/leak-scan.sh` instead checks that every
line the suite printed matches the fixed vocabulary it is allowed to print:

```bash
COSMIC_PASS_LIVE=1 just leak-scan
```

It reports `LEAK: the live suite printed lines outside its allowed vocabulary` and exits 1 if
the suite ever prints something nobody vetted. Exit 2 means a step was skipped, so the scan
proves nothing about that step.

## V7. The live suite refuses to run without a session

```bash
pass-cli logout
just test-live
```

**Expect**: red, with `not signed in; run: pass-cli login` — a message, not an assertion dump.
Sign back in afterwards.

## V8. Fixture shape drift is reported, not just detected

The check is asymmetric on purpose. A path in the fixture that fresh output no longer carries
means upstream removed a field, so the fixture is now fiction: **red**. A path in fresh output
that the fixture lacks is usually sampling chance -- `capture-fixtures` keeps only a couple of
items per kind -- so it is **reported and passes**.

Exercise the failing direction by adding a key the real `pass-cli` does not emit:

```bash
cp -f tests/fixtures/pass-cli/captured/vault-list.json /tmp/vault-list.bak
python3 -c "import json,pathlib; p=pathlib.Path('tests/fixtures/pass-cli/captured/vault-list.json'); d=json.loads(p.read_text()); [v.update(retired_upstream_field='x') for v in d['vaults']]; p.write_text(json.dumps(d))"
COSMIC_PASS_LIVE=1 cargo nextest run --all-features --run-ignored=only \
    -E 'binary(pass_cli_live) and test(committed_fixtures)' --no-capture
cp -f /tmp/vault-list.bak tests/fixtures/pass-cli/captured/vault-list.json
```

**Expect**: red, with
`FIXTURE vault-list.json: upstream no longer emits ["vaults[].retired_upstream_field (String)"]`.
The report lists key paths and value kinds, never values.

## V9. The coverage table stays honest

```bash
grep -c '^|' specs/003-pass-cli-contract-tests/contracts/pass-cli-test-harness.md
```

Then read the coverage table and confirm every clause of
`specs/001-quick-access-launcher/contracts/pass-cli.md` appears exactly once, as `contract`,
`live`, `already covered`, or `manual` with a reason (SC-001). This one is a manual review;
it is the check that keeps the automated coverage claim true.

## V10. Budget

```bash
nix develop -c cargo nextest run -E 'binary(pass_cli_contract)' 2>&1 | tail -3
```

**Expect**: total wall clock well under 30 s (SC-003).

---

## V11. A hung probe fails the suite instead of wedging it

```bash
cargo nextest run -E 'binary(pass_cli_contract) and test(timeout::)' --no-capture
```

**Expect**: green in about 2.4 s. The probe drives `sh -c 'sleep 2; : > marker'` with a 200 ms
deadline and asserts three things: the failure says the probe timed out and was killed, the call
returned long before the child would have finished, and the marker the child would write never
appears -- the last one is what separates "killed" from "abandoned" (FR-013).

To see it fail, delete the `child.kill()` call in `RawOutput::try_capture` and re-run: the marker
assertion goes red with `the child outlived the probe that spawned it`.

## V12. A probe reuses its kernel-keyring key instead of minting one

```bash
before=$(grep -c '@ProtonPassCLI' /proc/keys)
cargo nextest run -E 'binary(pass_cli_contract)'
echo "$before -> $(grep -c '@ProtonPassCLI' /proc/keys)"
```

**Expect**: the two counts are equal on every run after the first, and `grep '^ 1000' /proc/key-users`
stays flat. The first run on a fresh checkout creates seven keys -- one per probe home -- and
every run after that reuses them.

`pass-cli` encrypts its store with a key it keeps in the *persistent* kernel keyring, addressed
by the hash of the store path and marked `perm`. That keyring is per-uid, so no `HOME` override
reaches it, and `logout --force` does not clear it. Random homes therefore leaked six permanent
keys a run against a 200-key, 20000-byte per-user quota; when it filled, every probe failed with
`Error accessing keyring: Platform failure: QuotaExceeded`. Stable homes make the key repeat.

To see it fail, make `IsolatedEnv::new` return `Self::stateless()` and re-run
`keyring::a_probe_reuses_one_kernel_key`: it goes red with `a labelled home must land on the
same path every run`.

If a keyring ever does fill, the leaked keys are removable and nothing else is in that keyring:

```bash
keyctl unlink <id> $(keyctl get_persistent @s)   # for each id in `keyctl show` matching @ProtonPassCLI
```

---

## Recorded outcomes

Run on 2026-09-21, `pass-cli` 2.3.3 (`0d7235d`) from the pinned `nixpkgs-pass-cli` input.
Re-recorded the same day after Phase 7 added the timeout probe and the `-share-2` fixture
comparison.

| Item | Outcome |
|---|---|
| V1 contract suite in the dev shell | pass -- 23 contract tests, 373 in the whole `just test` run, no `SKIP` line |
| V2 contributor without `pass-cli` | pass -- green with `SKIP pass_cli_contract: no pass-cli on PATH and COSMIC_PASS_CLI unset` |
| V3 missing binary marked required | pass -- red, naming `COSMIC_PASS_CLI=/nonexistent/pass-cli, and COSMIC_PASS_REQUIRE_CLI=1 marks it required` |
| V4 SC-002 mutations | pass -- exactly one assertion red per mutation (see below) |
| V5 developer session untouched | pass -- still signed in after a full contract run; no fixture or tracked file changed; and repeated runs leave the kernel-keyring key count unchanged, which T057 fixed and `keyring::a_probe_reuses_one_kernel_key` now holds |
| V6 live suite | pass on the pre-Phase-7 suite -- 11 tests, ~57 s, every scenario `covered`; 2 vaults, 572 items, 2 TOTP fields. **Not re-run since**: the session is locked, so `live()` stops at `pass-cli is not usable: the Proton Pass session is locked`. The `fixtures compared:` count and the `SCENARIO fixture-shape-share-2` line are therefore unobserved (cosmic-pass-5kr) |
| V7 live suite with no session | **not run** -- it would have meant logging the developer out; the preflight path is `live()` in `tests/pass_cli_live.rs`, which panics with `not signed in; run: pass-cli login` |
| V8 fixture drift | pass -- both directions: a fixture-only path fails red, a fresh-only path is reported and passes |
| V9 coverage table review | pass -- every clause of the consumed contract appears once, as contract / live / already covered / manual |
| V10 budget | pass -- contract suite 2.412 s, of which 2.4 s is V11's deliberate sleep; the other twenty-two probes run in 0.665 s. Far inside the 30 s target |
| V11 hung probe killed | pass -- green in 2.409 s; removing `child.kill()` turns the marker assertion red, as documented above |
| V12 keyring key reuse | pass -- three consecutive whole-suite runs left the `@ProtonPassCLI` key count unchanged; before T057 each run added six |
| `just check` (whole gate) | pass -- 373 tests, line coverage 94.33% of 5981 lines |

### V4 detail

| Mutation in `src/pass/backend.rs` | Failing assertion |
|---|---|
| `--show-secrets` -> `--show-secret` | `argv::every_command_the_app_builds_is_accepted` |
| `item totp` -> `item otp` | `argv::every_command_the_app_builds_is_accepted` |
| `--share-id=<v>` -> space-separated | `argv::ids_must_use_the_equals_form` |

No collateral failures in any of the three.

### V6 latency, against the figures recorded in the consumed contract

| Command | Recorded | Measured |
|---|---|---|
| `info` | ~0.6 s | 0.80 s |
| `vault list` | ~0.6 s | 0.52 s |
| `item list --show-secrets`, all vaults | ~3.6 s (2 vaults, 616 items) | 4.14 s (2 vaults, 572 items) |
