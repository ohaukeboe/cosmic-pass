# Quickstart: validating "One surface for item fields"

Prerequisites: `nix develop` (or `direnv allow`) for the toolchain. A real Proton Pass account
is needed only for the manual pass; every automated check runs against fakes.

## Automated

```bash
just check          # fmt + clippy -D warnings + nextest + coverage floor
just test           # cargo nextest run, if you want the suite alone
```

Targeted runs while working:

```bash
cargo nextest run reveal                     # reducer + acceptance reveal tests
cargo nextest run --test story3_reveal_in_action_list
cargo nextest run --test story2_other_fields
cargo nextest run config                     # preference load/compat tests
INSTA_UPDATE=always cargo nextest run parse  # accept the parser snapshot after review
```

What each layer must prove (see [spec.md](./spec.md) for the requirement numbers):

| Layer | File | Proves |
|-------|------|--------|
| Reducer unit | `src/core/state.rs` tests | reveal follows the highlight, one at a time, cleared on move/leave, late value dropped (FR-101, FR-102, FR-103, FR-106) |
| Reducer unit | `src/core/actions.rs` tests | `field_index` points at the right field, including duplicate names and the repeated primary row (R1) |
| Parser snapshot | `src/pass/parse.rs` + `src/pass/snapshots/` | text and unrecognized custom fields absent, hidden and TOTP ones present (FR-113, FR-114) |
| Config unit | `src/config.rs` tests | a stored `open_detail` entry is dropped and every other chord survives; no `Ctrl+I` default remains (FR-109, FR-110) |
| View unit | `src/app/view/actions.rs` tests | revealed row renders plaintext, code row renders countdown, empty list states it (FR-104, FR-107, FR-117) |
| Key mapping | `src/app/keys.rs` tests | `Ctrl+R` maps in `Mode::Actions`, nothing maps to the removed action (FR-101, FR-108) |
| Acceptance | `tests/story3_reveal_in_action_list.rs` | Story 1 end to end through the harness |
| Acceptance | `tests/story2_other_fields.rs` | dropped fields are not copy targets and shortcuts still land on the right field (FR-116) |

## Manual (needs a signed-in `pass-cli`)

```bash
just run
```

1. Search an item with a password, open the field list with `Tab`.
2. Highlight the password row, press `Ctrl+R` — the value appears on that row only. Press
   `Ctrl+R` again — masked. Reveal again, then press `Down` — masked again.
3. Highlight the one-time-code row, press `Ctrl+R` — code plus a countdown that rolls over at the
   end of the period.
4. Highlight the username row, press `Ctrl+R` — nothing happens, no notice.
5. Press `Ctrl+I` from the result list — nothing opens.
6. Open preferences (`Ctrl+,`) — no "Show details" row; every chord you had set before the
   upgrade is still there.
7. Open an item that has a user-defined text field in Proton Pass — the field is absent, while
   its hidden and one-time-code siblings are listed.
8. Sign out (or pull the network) and press `Ctrl+R` on a secret row — the row stays masked and
   the notice line says why.

## Expected diff surface

One deletion (`src/app/view/detail.rs`), one test rename
(`tests/story3_detail_pane.rs` → `tests/story3_reveal_in_action_list.rs`), and edits in
`src/config.rs`, `src/model.rs`, `src/core/{actions,effects,state}.rs`, `src/pass/parse.rs`,
`src/app/keys.rs`, `src/app/view/{mod,actions}.rs`. Docs: this feature's contracts, the amended
001 contracts, and the README keys table if it grows a reveal row.
