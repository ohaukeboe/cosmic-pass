# Contract: Keyboard interaction

All actions work without a mouse (FR-004). Chords marked *configurable* can be changed in
Preferences (FR-027, [config.md](./config.md)). Defaults below.

## List mode (search field focused)

| Key | Action | Configurable |
|-----|--------|--------------|
| any text | Edit query; results update per keystroke | no |
| `Down`, `Ctrl+N`, `Ctrl+J` | Select next result | no |
| `Up`, `Ctrl+P`, `Ctrl+K` | Select previous result | no |
| `Page Down` / `Page Up` | Move selection by 10 | no |
| `Enter` | Copy primary field, close window | yes (`copy_primary`) |
| `Ctrl+U` | Copy username (or email if no username), close | yes (`copy_username`) |
| `Ctrl+O` | Copy current one-time code, close | yes (`copy_totp`) |
| `Ctrl+L` | Copy first website, close | yes (`copy_url`) |
| `Tab`; `Right` when the query is empty | Open action list | yes (`open_actions`) |
| `Ctrl+I` | Open detail pane | yes (`open_detail`) |
| `F5` | Refresh data | yes (`refresh`) |
| `Ctrl+,` | Open preferences | yes (`preferences`) |
| `Ctrl+Shift+S` | Sign in to Proton Pass (status panel) | yes (`sign_in`) |
| `Ctrl+Shift+R` | Retry after an error, lock, or missing `pass-cli` (status panel) | yes (`retry`) |
| `Escape` | If a fetch is pending: cancel it. Else: close window | no |

When the requested field is missing (e.g. no TOTP), show an inline notice for 3 s and leave
the clipboard unchanged.

## Action list mode

Lists every copyable field of the selected item with its shortcut. Secret values are masked.

| Key | Action |
|-----|--------|
| `Up` / `Down` | Move |
| `Enter` | Copy highlighted field, close window |
| action chords from list mode | Same as list mode |
| `Escape`, `Left`, `Shift+Tab` | Back to list mode |

## Detail pane mode

Shows title, vault, kind, username, websites, masked password, TOTP code with countdown.

| Key | Action | Configurable |
|-----|--------|--------------|
| `Ctrl+R` | Toggle reveal of secret fields | yes (`reveal`) |
| action chords from list mode | Copy, close window | yes |
| `Escape` | Back to list mode (drops revealed values) | no |

The status panel replaces the result list while the session is signed out, locked, in error,
or `pass-cli` is missing. Its buttons carry the same two chords, which are ignored when the
panel is not shown (signing in twice, or refreshing a healthy session, does nothing).

## Preferences mode

The editor has no focus ring — Tab traversal is off — so every control has a chord. The list
of shortcut rows is not a selectable list: a row is reached by pressing the chord it shows.

| Key | Action | Configurable |
|-----|--------|--------------|
| `Right`, `Up`, `+`, `=` | Clipboard timeout +10 s | no |
| `Left`, `Down`, `-`, `_` | Clipboard timeout −10 s | no |
| any bound chord (e.g. `Ctrl+U`) | Start rebinding that action's row | no |
| `Ctrl+Shift+Delete` | Restore every shortcut to its default | no |
| `Escape` | Back to list mode | no |

`Page Up` / `Page Down` and the list-navigation chords (`Ctrl+N`, `Ctrl+P`, …) do nothing
here: the result list is hidden, so its selection must not move.

While a row waits for its new chord ("Press keys…"), capture takes precedence: every key but
`Escape` (cancel) and a lone modifier becomes the new binding, including the keys above.

## Mouse

- Click result: select. Double-click: copy primary.
- Click action in action list: copy.
- Click outside window: window loses focus and hides.

## Focus and window rules

- Window opens with the search field focused and empty (FR-002, FR-003).
- Losing keyboard focus hides the window.
- Pressing the global shortcut while visible hides it (toggle).
