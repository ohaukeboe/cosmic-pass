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
| `revealed: Revealed` | **replaces** `revealed`, `revealed_field`, `totp` and `totp_fetching` | One slot for whatever the field list shows unmasked. See the enum below. |
| `reveal_cancel: Option<CancellationToken>` | unchanged | Own token so re-masking a field cannot cancel a code fetch. |
| `detail_cancel: Option<CancellationToken>` | **renamed** to `reveal_totp_cancel` | Same role; the name follows the surviving surface. |

### `core::state::Revealed` (added 2026-09-20, bd `cosmic-pass-39x`)

| Variant | Carries | Notes |
|---------|---------|-------|
| `Nothing` | — | Every row is masked. The default. |
| `Field { target, value }` | `RevealTarget`, `Option<SecretString>` | `target` pins item, position, field and `modified_at`; a value arriving for a stale target is dropped (FR-106). `value` is `None` until the fetch lands, which is what the row's in-flight marker reads. |
| `Totp { code, fetching }` | `Option<TotpDisplay>`, `bool` | `code` is kept while `fetching` is set, so an expiring code stays on screen until its replacement arrives. |

Read through `Revealed::value()`, `field_target()`, `totp_code()` and `totp_fetching()`, or
through `Model::revealed_value(index)`, `revealed_totp()`, `revealing_field(index)` and
`revealing_totp()`.

**Invariant (FR-102)**: at most one value is revealed at a time. This is now a property of the
type — a field reveal and a code cannot both be stored — rather than a rule `clear_revealed()`
has to hold across two stores.

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
