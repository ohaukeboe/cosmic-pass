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

**Also verify**: nothing in the output is a real secret. Scan it:

```bash
just test-live 2>&1 | tee "$TMPDIR/live.log"
just leak-scan
grep -c 'SECRET' "$TMPDIR/live.log"      # expect 0 outside fixture placeholder names
```

## V7. The live suite refuses to run without a session

```bash
pass-cli logout
just test-live
```

**Expect**: red, with `not signed in; run: pass-cli login` — a message, not an assertion dump.
Sign back in afterwards.

## V8. Fixture shape drift is reported, not just detected

Hand-edit one key out of a committed fixture, run the live suite, then restore it:

```bash
cp -f tests/fixtures/pass-cli/captured/vault-list.json "$TMPDIR/vault-list.json.bak"
# remove one key, e.g. vaults[].vault_id
just test-live
cp -f "$TMPDIR/vault-list.json.bak" tests/fixtures/pass-cli/captured/vault-list.json
```

**Expect**: red, naming `vaults[].vault_id` as present in fresh output and missing from the
fixture. The report must list key paths, never values.

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
