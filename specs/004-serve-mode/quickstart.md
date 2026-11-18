# Quickstart: validating `cosmic-pass serve`

Run inside the dev shell (`nix develop` or direnv).

## Automated

```bash
just test       # includes tests/serve_protocol.rs and the protocol unit tests
just bench      # includes search_bench_serve_query (SC-001: query snapshot < 100 ms, 2,000 items)
just check      # all pre-merge gates
```

Expected: all green; coverage stays at or above 80%.

## Manual, against the fake CLI (no account needed)

```bash
export COSMIC_PASS_CLI=$PWD/tests/fixtures/fake-pass-cli COSMIC_PASS_NO_KEYRING=1 \
  COSMIC_PASS_CACHE_DIR=$(mktemp -d) FAKE_ITEMS=20
cargo run -q -- serve --clipboard-clear-secs 5; echo "exit $?"
```

S1. Expected: no stdout, a usage error on stderr, `exit 2`.

```bash
printf '%s\n' '{"t":"show"}' '{"t":"query","text":"git"}' 'garbage' '{"t":"next"}' \
  | cargo run -q -- serve --clipboard-clear-secs 90; echo "exit $?"
```

S2. Expected: several JSON lines on stdout, each with `"v":1`; the line after `query` has
`"query":"git"`; `garbage` produces one stderr log line and no exit; `exit 0` when input ends.

S3. `cargo run -q -- serve --clipboard-clear-secs 90 | head -1` then type nothing: one snapshot
line appears before any input is read (FR-003, FR-006).

## Manual, real account (optional)

With `pass-cli login` done and a Wayland session:

```bash
cargo run -q -- serve --clipboard-clear-secs 30
```

Type `{"t":"show"}`, `{"t":"query","text":"<an item>"}`, `{"t":"copy_primary"}`, one per line.
Expected: the last snapshot has `"visible":false`, the password pastes elsewhere, and is gone from
the clipboard after 30 s. Grep the terminal scrollback for the password: it is not there.

The front end's end-to-end checks live in dot-emacs
`specs/016-proton-pass-quick-access/quickstart.md`.
