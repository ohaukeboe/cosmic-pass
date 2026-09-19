# Contract: Keyboard interaction

All actions work without a mouse (FR-004). Chords marked *configurable* can be changed in
Preferences (FR-027, [config.md](./config.md)). Defaults below.

**Amended 2026-09-19 by feature 002** (one surface for item fields): the detail pane is gone
along with its `Ctrl+I` chord, and reveal moved onto the highlighted row of the action list.
The rows below already reflect that; the full amendment, including the reveal rules it adds,
is in
[002-action-list-reveal/contracts/keyboard.md](../../002-action-list-reveal/contracts/keyboard.md).

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
| `Ctrl+R` | Nothing here: reveal acts on an action-list row | yes (`reveal`) |
| `F5` | Refresh data | yes (`refresh`) |
| `Ctrl+,` | Open preferences | yes (`preferences`) |
| `Ctrl+Shift+S` | Sign in to Proton Pass (status panel) | yes (`sign_in`) |
| `Ctrl+Shift+R` | Retry after an error, lock, or missing `pass-cli` (status panel) | yes (`retry`) |
| `Escape` | If a fetch is pending: cancel it. Else: close window | no |

When the requested field is missing (e.g. no TOTP), show an inline notice for 3 s and leave
the clipboard unchanged.

The status panel replaces the result list while the session is signed out, locked, in error,
or `pass-cli` is missing. Its buttons carry the same two chords, which are ignored when the
panel is not shown (signing in twice, or refreshing a healthy session, does nothing).

## Action list mode

Lists every copyable field of the selected item with its shortcut, one row per field, plus one
row per one-time code. Secret values are masked; user-defined text fields are not listed at all
(feature 002). When the item has no listable field, the screen says so instead of showing an
empty list.

| Key | Action | Configurable |
|-----|--------|--------------|
| `Up` / `Down`, `Ctrl+P` / `Ctrl+N`, `Ctrl+K` / `Ctrl+J`, `Page Up` / `Page Down` | Move the highlight; masks anything revealed | no |
| `Enter` | Copy the highlighted field, close window | yes (`copy_primary`) |
| `Ctrl+R` | Reveal the highlighted row in place; press again to mask | yes (`reveal`) |
| action chords from list mode | Same as list mode | yes |
| `Escape`, `Left`, `Shift+Tab` | Back to list mode (drops revealed values) | no |

`Escape` cancels a pending fetch first, as in list mode, and only then goes back.

Reveal rules:

1. At most one row is revealed at a time.
2. A secret row shows its plaintext; a one-time-code row shows the current code and the seconds
   left in its period, refreshing when the period ends.
3. A row whose value is already on screen ignores the key — no change, no notice.
4. The value is fetched at the moment of the press, never before, and is dropped on mask,
   highlight move, leaving the screen, or window close.
5. A fetch that fails leaves the row masked and reports why on the existing notice line.
6. A value that arrives after its field has moved, been renamed or been removed is discarded.

The footer names the chords that act on the highlighted row, so the reveal chord is discoverable
without this document.

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
