# Phase 0 Research: One surface for item fields

No `NEEDS CLARIFICATION` markers entered this phase — the two open scope questions were settled
with the user during `/speckit-specify` and recorded in the spec's Assumptions. What follows are
the design decisions the implementation needs, each checked against the code as it stands.

---

## R1 — How a reveal finds the field behind an action row

**Decision**: `ActionEntry` gains `field_index: Option<usize>` — the row's position in
`item.fields`, or `None` for a one-time-code row. `Msg::ToggleReveal` reads the highlighted
row's `field_index` and pins a `RevealTarget` exactly as the detail pane does today.

**Rationale**: The reveal machinery already pins `(generation, key, index, field, modified_at)`
and re-checks it on show (`Model::revealed_value`, `RevealTarget::still_describes`), which is
what makes a late value safe (FR-106). Row order and field order differ — `all_actions` emits
the primary field first, then the remaining fields, then TOTP rows — so the row index cannot be
used as a field index, and the view already renders values by field index. Carrying the position
on the entry keeps one source of truth and costs one `usize`.

**Alternatives considered**:

- *Look the field up by name*: breaks for items with two fields of the same name, which
  `all_actions` deliberately disambiguates by position today.
- *Store the field index inside `CopySource::Field`*: `CopySource` is also produced by
  `primary_action`/`username_action`/`url_action` for list-mode copies where no row exists; an
  index there would be meaningless half the time.
- *Re-derive the index in the view*: the reducer is where the target is pinned, and `src/core`
  must stay the single owner of that decision.

---

## R2 — Revealing a one-time-code row

**Decision**: Revealing a TOTP row fetches through the existing `Effect::FetchTotp` and stores
the result in `view.totp` (a `TotpDisplay`, which already carries the field name, code and
`valid_until`). The action list renders code + remaining seconds on that row. `Msg::Tick` keeps
refreshing it, but only while a TOTP row is revealed. Masking, moving the highlight, leaving the
list or closing the window clears `view.totp` and cancels the fetch.

**Rationale**: FR-104 carries FR-019 over from the removed pane, and the countdown, refresh and
cancellation already exist; only the trigger moves from "pane opened" to "row revealed". Fetching
on reveal rather than on opening the list also removes a `pass-cli` call the detail pane made
every time it opened.

**Alternatives considered**:

- *Keep fetching the code whenever the list opens*: a network/subprocess call for an item the
  user may only want the username of; the detail pane's behavior, not worth keeping.
- *Treat the code like any other secret and route it through `FetchReveal`*: `FetchReveal`
  returns one value with no expiry, so the countdown and auto-refresh would have to be rebuilt.

---

## R3 — "At most one revealed at a time" with two stores

**Decision**: `Msg::ToggleReveal` always clears **both** stores first (`drop_reveal()` and the
TOTP display), then re-arms whichever the highlighted row needs. Moving the highlight
(`Msg::SelectNext`/`SelectPrev`/`PageUp`/`PageDown` in `Mode::Actions`) clears both.

**Rationale**: FR-102 is a single invariant over two pieces of state; concentrating it in one
helper (`clear_revealed()`, the renamed `clear_detail_secrets`) keeps it testable and stops a
field reveal and a code from being on screen together.

**Alternatives considered**: a single enum `Revealed { Field(..), Totp(..), None }` would encode
the invariant in the type. Rejected for now under YAGNI: it rewrites the TOTP refresh path for no
behavior the spec asks for, and the invariant is one line either way. Noted as a follow-up if a
third revealable kind ever appears.

**Amended 2026-09-20** (bd `cosmic-pass-39x`): the follow-up was taken ahead of a third kind.
`ViewState` now holds one `Revealed { Nothing, Field { target, value }, Totp { code, fetching } }`
in place of `revealed`, `revealed_field`, `totp` and `totp_fetching`, so FR-102 is a property of
the type rather than of `clear_revealed()`. Behavior is unchanged: the two cancellation tokens,
the pinning rules and the refresh path all stayed as described above, and `Revealed::Totp` keeps
its `code` while `fetching` is set so an expiring code stays on screen until its replacement
lands.

---

## R4 — Preferences must survive a stored `open_detail` binding

**Decision**: Give `Preferences::shortcuts` a tolerant load: a `Shortcuts` newtype whose
`Deserialize` reads string keys, maps the ones it recognizes onto `Action`, and silently drops
the rest. `Action::OpenDetail` is deleted outright.

**Rationale**: Verified against the generated code in `cosmic-config-derive` (`get_entry` builds
`Self::default()` and overwrites one field per config key, collecting errors). A stored
`shortcuts` value containing `open_detail` fails `Action`'s derived `Deserialize` with
"unknown variant", the whole field is left at its default, and **every** user-set chord reverts —
exactly what FR-110 forbids. The same tolerance protects any future action rename, and
`Preferences::validated()` already rebuilds the map from `Action::ALL`, so an unknown key is
dropped one step later anyway.

**Alternatives considered**:

- *Keep `Action::OpenDetail` as a hidden variant outside `Action::ALL`*: parses, but leaves a
  variant with a label and a default chord that nothing can reach — dead code (Principle I).
- *Bump the config `#[version]` to 2*: cosmic-config would read a fresh directory, silently
  discarding every preference, not just the stale one.
- *Do nothing and accept the reset*: violates FR-110 and SC-005.

---

## R5 — Where user-defined text fields are dropped

**Decision**: Drop them in `pass::parse::custom_fields`: `CustomFieldKind::Text` and
`CustomFieldKind::Other` stop producing a `FieldRef`. `Hidden` (secret) and `Totp` are untouched.

**Rationale**: The parser is the one place every consumer reads from — action list, copy
shortcuts, primary-field choice and the on-disk cache. Filtering in the view would leave the
fields in the cache and still let a shortcut resolve to one. `Other` covers content variants this
version does not know; the app can show or copy them just as little as `Text`, so they go the
same way (FR-113). Built-in fetch-on-demand members (`phone_number`, `first_name`, …) are created
by `unstored_if`, are untouched, and keep working (FR-115).

**Alternatives considered**:

- *Filter in `core::actions::all_actions`*: fixes the list, not the cache, and leaves
  `primary_action` able to pick a field that can never be copied.
- *Keep the fields and try the fetch anyway*: the fetch is what fails; this is the bug being
  removed, not a behavior to preserve.

---

## R6 — Existing caches that already contain the dropped fields

**Decision**: Bump `CACHE_FORMAT_VERSION` from 1 to 2.

**Rationale**: `ItemSummary` is cached with `postcard`; an existing cache still holds the dead
rows and the list is drawn from the cache before the background refresh lands, so without a bump
the first window after upgrade shows exactly the rows this feature removes. `store.rs` already
discards a cache whose `format_version` differs, and `cache-format.md` already specifies that
behavior — the bump costs one refresh, once.

**Alternatives considered**: filtering dead fields on cache load — a second filter to keep in
sync with the parser, for one startup's benefit.

---

## R7 — What replaces the detail pane's reveal cycle

**Decision**: Reveal follows the highlight. `Ctrl+R` on a non-secret row is a no-op with no
notice (FR-105); `Ctrl+R` in list mode or preferences stays unbound.

**Rationale**: The pane had no highlight, so cycling was the only way to address a field; the
action list has one. Cycling on top of a highlight would be two selections for one screen.
A no-op on an already-visible value is quieter than an inline notice for something the user can
see is already shown.

**Alternatives considered**: *skip to the next secret row when the highlighted one is not
secret* — moves the selection out from under the user as a side effect of a reveal press.

---

## R8 — What happens to the old story-3 acceptance test

**Decision**: Rewrite `tests/story3_detail_pane.rs` as
`tests/story3_reveal_in_action_list.rs`, keeping every assertion that still describes required
behavior (reveal toggles, one at a time, dropped on leaving, late value discarded, failed fetch
reports and stays masked) and re-pointing them at `Mode::Actions`.

**Rationale**: Principle II forbids deleting tests to make a build pass. These tests cover FR-101
through FR-106, which survive the move; only their entry point changes. The two assertions that
are genuinely about the removed screen (`opening_detail_fetches_totp_only_when_present`) are
replaced by the reveal-triggered equivalent.
