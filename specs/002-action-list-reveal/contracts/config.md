# Contract: Preferences (amended by feature 002)

Amends [feature 001's config contract](../../001-quick-access-launcher/contracts/config.md).
Config ID, version, location and the three numeric keys are unchanged.

## `shortcuts`

`Action` names after this feature:

`copy_primary`, `copy_username`, `copy_totp`, `copy_url`, `open_actions`, `reveal`, `refresh`,
`preferences`, `sign_in`, `retry`.

`open_detail` is **removed**.

| Situation | Behavior |
|-----------|----------|
| Stored map contains `open_detail` | That entry is ignored. Every other stored chord is kept, and `Ctrl+I` ends up bound to nothing unless the user assigns it. No warning is required, and start-up must not fail. |
| Stored map contains any other unrecognized action name | Same: entry dropped, rest kept. |
| Recognized action missing from the stored map | Filled with its default chord (unchanged rule). |
| Duplicate chords | Later duplicate falls back to its default (unchanged rule). |

`KeyChord` RON form is unchanged: `(modifiers: [Ctrl, Shift], key: "u")`. The map's serialized
shape is unchanged, so a config written by this version is still readable by the previous one
(minus the removed action).

## `reveal`

`reveal` keeps its `Ctrl+R` default but now applies in the field list instead of the removed
detail pane. No migration is performed for users who rebound it.

## Cache

`CACHE_FORMAT_VERSION` moves from `1` to `2`
([cache-format.md](../../001-quick-access-launcher/contracts/cache-format.md) rules unchanged):
an existing cache is deleted on load and rebuilt by the next refresh, so no item keeps a
user-defined text field that this version no longer lists.
