# Contract: Preferences (cosmic-config)

- Config ID: `io.github.ohaukeboe.CosmicPass`
- Version: `1`
- Location: `~/.config/cosmic/io.github.ohaukeboe.CosmicPass/v1/<key>` (one RON value per file)
- Changes are watched and applied live.

| Key | Type | Default | Valid range / rule |
|-----|------|---------|--------------------|
| `clipboard_clear_secs` | `u32` | `90` | 10–600. Out-of-range values are clamped and a warning is logged. |
| `refresh_stale_secs` | `u32` | `300` | 30–86400 |
| `max_results` | `u16` | `12` | 5–200. Each row costs layout and draw time; large values make typing sluggish. |
| `shortcuts` | map of `Action` to `KeyChord` | see [keyboard.md](./keyboard.md) | Duplicate chords rejected in the UI; on load, later duplicates fall back to default. An entry naming an action this version does not know is dropped and every other entry is kept. |

`Action` names: `copy_primary`, `copy_username`, `copy_totp`, `copy_url`, `open_actions`,
`reveal`, `refresh`, `preferences`, `sign_in`, `retry`.

**Amended 2026-09-19 by feature 002** (one surface for item fields): `open_detail` is removed,
so `Ctrl+I` is bound to nothing and a stored `open_detail` entry is one of the unknown names
dropped on load. Dropping only that entry is what keeps the rest: cosmic-config overwrites one
field per stored key, so a `shortcuts` value that failed to parse would revert every chord the
user ever set. Full amendment:
[002-action-list-reveal/contracts/config.md](../../002-action-list-reveal/contracts/config.md).

`KeyChord` RON form: `(modifiers: [Ctrl, Shift], key: "u")`.

Invalid or unreadable config never blocks startup: the app uses defaults and logs a warning.
No secret or item data is ever stored in config.
