# Quickstart & Validation: Proton Pass Quick Access

**Feature**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md)

This guide proves the feature works end to end. It references contracts instead of repeating
them.

## Prerequisites

- COSMIC desktop session on Wayland.
- `nix` with flakes enabled, plus `direnv` (the repo `.envrc` runs `use flake`), or run
  `nix develop` manually.
- `pass-cli` 2.3.x on `PATH`, signed in (`pass-cli login`) with at least one vault holding:
  a login with TOTP, a login without TOTP, a credit card, a secure note, and two logins with
  the same title in different vaults.
- A Secret Service provider running (e.g. gnome-keyring), unlocked.

## Build and quality gates

```bash
nix develop          # or: direnv allow
just check           # fmt + clippy -D warnings + nextest + coverage floor
just run             # release build (debug builds render far too slowly)
nix build            # the packaged binary, with the test suite run in the sandbox
```

Expected: every gate passes with zero warnings.

## Install for manual testing

```bash
just install-user    # binary to ~/.local/bin, unit + desktop file to ~/.local/share
systemctl --user daemon-reload
systemctl --user enable --now cosmic-pass.service
```

Add the shortcut: COSMIC Settings → Keyboard → Keyboard Shortcuts → Custom → Add
→ command `cosmic-pass`, keys `Super+Shift+P`.

## Validation scenarios

### V1. Capture real `pass-cli` output (first implementation task)

```bash
just capture-fixtures   # runs the commands in contracts/pass-cli.md, redacts values,
                        # writes tests/fixtures/pass-cli/*.json
```

Expected: fixtures parse with `cargo nextest run pass::parse`. Review the redacted files
before committing: no real titles, usernames, URLs, or secrets.
Also record the signed-out and locked `Error:` lines.

### V2. Core flow — User Story 1

1. Press `Super+Shift+P`. Window appears centered, search focused (SC-001).
   To record the number: stop the user service, run
   `RUST_LOG=cosmic_pass=debug ./target/release/cosmic-pass --background 2>/tmp/open-latency.log`,
   open and close the popup ~20 times, then take the p95 of the `open latency:` lines and record
   it in research.md ("Open latency"). That figure covers the resident instance only; add the
   shortcut's own process spawn and D-Bus hop by timing `time cosmic-pass` while an instance runs.
2. Type part of the TOTP login's title. Results narrow on each keystroke.
3. Press `Enter`. Window closes.
4. Paste into a text editor. Value equals the item's password.
5. Wait 90 s. Paste again. Clipboard is empty.
6. Repeat 1–3, then copy some other text before 90 s pass. After 90 s the other text is
   still on the clipboard (FR-015).
7. Open, type text, press `Escape`. Reopen: query is empty (FR-003).
8. Open, click another window. Window hides.

### V3. Other fields — User Story 2

1. Select the TOTP login, press `Ctrl+O`; paste matches the code in Proton Pass.
2. Select the login without TOTP, press `Ctrl+O`; inline "no one-time code" notice,
   clipboard unchanged.
3. Press `Tab` on the credit card; action list shows card fields masked. Copy the number.
4. `Enter` on the note copies the note body.

### V4. Clipboard safety

```bash
wl-paste --list-types     # right after a password copy
```

Expected: includes `x-kde-passwordManagerHint`. After the timeout, `wl-paste` reports no
selection (validates research V3). URL/username copies do not start a timeout.

### V5. Status handling — User Story 4

1. `pass-cli logout`, open window → "Not signed in" + "Sign in" action. The cache file
   `~/.cache/cosmic-pass/cache.bin` is gone.
2. Choose "Sign in" → browser opens (or URL shown); after completing, results load.
3. `COSMIC_PASS_CLI=/nonexistent cosmic-pass --background` (after stopping the service),
   open window → message naming `pass-cli` and where to get it.
4. Disconnect network, press `F5` → cached results remain, stale indicator shown.

### V6. Detail pane — User Story 3

1. `Ctrl+I` on the TOTP login → fields shown, password masked, code with countdown.
2. `Ctrl+R` reveals password. `Escape`, reopen pane → masked again.
3. Wait for the countdown to end → new code appears without input.
4. Check whether `iced_test` drives these flows headlessly (research V2); record the result
   in `research.md`.

### V7. Cache and keyring

1. Reboot (or `systemctl --user restart cosmic-pass`), open window immediately → results
   show before the refresh finishes.
2. Lock the keyring, restart the service → no `cache.bin` is written; app still works.
3. `COSMIC_PASS_NO_KEYRING=1` behaves the same.

### V8. Performance and no-leak checks

```bash
just bench          # search over 5,000 generated items: p95 per keystroke < 50 ms (SC-002)
just leak-scan      # runs integration suite, then greps all files the app wrote
                    # (cache dir, config dir, journal) for every fixture secret (SC-006)
```

Expected: benchmark passes; leak scan finds zero matches.

### V9. Large account responsiveness

With the fake CLI serving 5,000 items and a 3 s delay
(`FAKE_ITEMS=5000 FAKE_SLEEP=3 COSMIC_PASS_CLI=tests/fixtures/fake-pass-cli just run`), type
continuously while the refresh runs. No keystroke is dropped or delayed (SC-004).
