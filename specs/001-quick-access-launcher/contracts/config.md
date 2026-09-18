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
| `shortcuts` | `BTreeMap<Action, KeyChord>` | see [keyboard.md](./keyboard.md) | Duplicate chords rejected in the UI; on load, later duplicates fall back to default. |

`Action` names: `copy_primary`, `copy_username`, `copy_totp`, `copy_url`, `open_actions`,
`open_detail`, `reveal`, `refresh`, `preferences`.

`KeyChord` RON form: `(modifiers: [Ctrl, Shift], key: "u")`.

Invalid or unreadable config never blocks startup: the app uses defaults and logs a warning.
No secret or item data is ever stored in config.
