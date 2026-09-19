# Phase 1 Data Model: One surface for item fields

Deltas only. Everything not listed keeps the shape given in
[feature 001's data model](../001-quick-access-launcher/data-model.md).

## `core::actions::ActionEntry` (changed)

| Field | Type | Change | Notes |
|-------|------|--------|-------|
| `source` | `CopySource` | — | unchanged |
| `label` | `String` | — | unchanged |
| `shortcut` | `Option<Action>` | — | unchanged |
| `field_index` | `Option<usize>` | **added** | Position of this row's field in `item.fields`; `None` for a one-time-code row. Set by `all_actions`, including for the primary row, which repeats a field that also appears later. |

**Rule**: `field_index` is a position in the item as it stood when the entry was built. It is
never persisted and never outlives a redraw; the reveal pins its own copy of the field
(`RevealTarget`) at the moment the reveal is requested.

## `core::state::Mode` (changed)

| Variant | Change |
|---------|--------|
| `List` | unchanged |
| `Actions { key, selected }` | unchanged shape; now the only per-item screen |
| `Detail { key }` | **removed** |
| `Preferences { rebinding }` | unchanged |

`app::view::Pane::Detail` is removed with it.

## `core::state::Msg` (changed)

| Message | Change |
|---------|--------|
| `OpenDetail` | **removed** |
| `ToggleReveal` | kept; now means "reveal or mask the highlighted action row" |
| `RevealFetched { key, index, generation, result }` | unchanged shape; guarded against `Mode::Actions` for `key` instead of `Mode::Detail` |
| `TotpFetched { key, result }` | unchanged shape; same guard change |

## `core::state::ViewState` (changed)

| Field | Change | Notes |
|-------|--------|-------|
| `revealed: Option<SecretString>` | unchanged | Plaintext of the revealed field. Cleared on mask, highlight move, leaving the list, window close. |
| `revealed_field: Option<RevealTarget>` | unchanged | Pins item, position, field and `modified_at`; a value arriving for a stale target is dropped (FR-106). |
| `reveal_cancel: Option<CancellationToken>` | unchanged | Own token so re-masking a field cannot cancel a code fetch. |
| `totp: Option<TotpDisplay>` | unchanged shape | Now set only while a one-time-code row is revealed, not on opening a screen. |
| `totp_fetching: bool` | unchanged | |
| `detail_cancel: Option<CancellationToken>` | **renamed** to `reveal_totp_cancel` | Same role; the name follows the surviving surface. |

**Invariant (FR-102)**: at most one of `revealed` / `totp` is `Some` at any time. Both are
cleared before either is re-armed.

## `config::Action` (changed)

| Variant | Change |
|---------|--------|
| `OpenDetail` | **removed**, along with its label "Show details" and its `Ctrl+I` default |
| all others | unchanged |

`Action::ALL` drops to 10 entries; the preferences editor and `action_for` follow it without
change.

## `config::Preferences::shortcuts` (changed)

| Aspect | Before | After |
|--------|--------|-------|
| Type | `BTreeMap<Action, KeyChord>` | `Shortcuts` newtype wrapping the same map |
| Load of an unknown action name | whole field fails → every chord reverts to default | unknown key dropped, every recognized chord kept (FR-110) |
| Save | unchanged RON shape (`{"copy_primary": (modifiers: [Ctrl], key: "u"), …}`) | unchanged — the newtype serializes as the same map |

## `model::CACHE_FORMAT_VERSION` (changed)

`1` → `2`. An existing cache is discarded on load and rebuilt by the background refresh, so no
cached item can still carry a dropped text field (R6).

## `ItemSummary.fields` (content rule, shape unchanged)

| Source in Proton Pass | Before | After |
|------------------------|--------|-------|
| Custom field, `Text` content | `FieldRef::unstored(name, name)` | **not emitted** |
| Custom field, unrecognized content | `FieldRef::unstored(name, name)` | **not emitted** |
| Custom field, `Hidden` content | `FieldRef::secret` | unchanged |
| Custom field, `Totp` content | pushed to `totp_fields` | unchanged |
| Built-in member fetched on demand (`phone_number`, `first_name`, …) | `FieldRef::unstored` | unchanged |
