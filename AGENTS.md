# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context,
and see the `beads` skill at `.agents/skills/beads/SKILL.md` (project install) or
`~/.agents/skills/beads/SKILL.md` (global install) for workflow guidance.

> **Architecture in one line:** Issues live in a local Dolt database
> (`.beads/embeddeddolt/`); cross-machine sync uses `bd dolt push/pull` (a
> git-compatible protocol), stored under `refs/dolt/data` on your git
> remote — separate from `refs/heads/*` where your code lives.
> `.beads/issues.jsonl` is a passive export, not the wire protocol.
>
> See [SYNC_CONCEPTS.md](https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md)
> for the one-screen overview and anti-patterns (don't treat JSONL as the
> source of truth; don't `bd import` during normal operation; don't
> reach for third-party Dolt hosting before trying the default).

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:970c3bf2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   bd dolt push
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->

## Build & Test

All tooling comes from the flake dev shell. Enter it with `nix develop` or `direnv allow`.

```bash
just check    # all pre-merge gates: fmt, clippy -D warnings, nextest, coverage >= 80%
just fmt      # cargo fmt --check
just lint     # cargo clippy --all-targets --all-features -- -D warnings
just test     # cargo nextest run
just cov      # coverage via cargo llvm-cov (main.rs and libcosmic view glue excluded)
just run      # run the app (pass extra args after `run`)
just bench    # search benchmark (release build, ignored tests)
just test-live # drive a real, signed-in pass-cli (read-only); NOT part of `check`
```

### Testing against the real `pass-cli`

`pass-cli` publishes no stability policy and has broken this app's assumptions inside patch
releases, so two suites drive the real binary rather than `tests/fixtures/fake-pass-cli`:

- `tests/pass_cli_contract.rs` runs in `just test` and `just check`. It needs no account, no
  network and no D-Bus: every probe runs against a throwaway home. The dev shell supplies the
  pinned `pass-cli`, so it always runs there; outside the shell, with no `pass-cli` on `PATH`,
  it prints `SKIP pass_cli_contract: ...` and passes.
- `tests/pass_cli_live.rs` runs only via `just test-live`. It needs `pass-cli login` first and
  reads the account you are signed into -- read-only, and it never prints a secret. Scenarios
  it cannot exercise (no TOTP item, no field inside a section) report
  `SCENARIO <name>: not covered: <reason>` instead of failing. Worth running before a release
  and after bumping the `nixpkgs-pass-cli` flake input.

`COSMIC_PASS_CLI=/path/to/pass-cli` points both suites at a specific binary, the same variable
the app itself reads. Which clause of the consumed interface each test defends is tabulated in
[`specs/003-pass-cli-contract-tests/contracts/pass-cli-test-harness.md`](specs/003-pass-cli-contract-tests/contracts/pass-cli-test-harness.md).

## Architecture Overview

Single Rust crate (edition 2024, libcosmic/iced) for COSMIC. A resident process shows a
layer-shell popup on D-Bus activation. `src/core` holds pure logic (reducer, search,
actions); `src/app` is a thin libcosmic adapter; `src/pass`, `src/cache`, and `src/clipboard`
are IO boundaries behind traits (`pass-cli` subprocesses, encrypted metadata cache,
clipboard helper process). Packaged as a flake: `nix build`, `nix run`, `nix profile install`. Design docs:
`specs/001-quick-access-launcher/`.

## Conventions & Patterns

- Follow `.specify/memory/constitution.md` (test-first, no warnings, simplicity).
- Never log, `Debug`-print, serialize, or pass in argv any secret; use `secrecy::SecretString`.
- `src/core` must not import libcosmic or do IO.
