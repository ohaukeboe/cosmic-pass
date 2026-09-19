# Contract: Keyboard interaction (amended by feature 002)

Amends [feature 001's keyboard contract](../../001-quick-access-launcher/contracts/keyboard.md).
Only the modes below change; every other row of that contract still holds. When this feature
ships, the 001 contract is updated in place to match and keeps pointing here for the history.

## List mode — changed rows

| Key | Action | Configurable |
|-----|--------|--------------|
| `Tab`; `Right` when the query is empty | Open the field list | yes (`open_actions`) |
| ~~`Ctrl+I`~~ | ~~Open detail pane~~ — **removed**; `Ctrl+I` is bound to nothing | — |
| `Ctrl+R` | Nothing in list mode (reveal is a field-list action) | yes (`reveal`) |

## Field list mode (was "action list mode")

Lists every copyable field of the selected item with its shortcut, one row per field, plus one
row per one-time code. Secret values are masked; user-defined text fields are not listed at all.
When an item has no listable field, the screen says so instead of showing an empty list.

| Key | Action | Configurable |
|-----|--------|--------------|
| `Up` / `Down`, `Page Up` / `Page Down` | Move the highlight; masks anything revealed | no |
| `Enter` | Copy the highlighted field, close the window | yes (`copy_primary`) |
| `Ctrl+R` | Reveal the highlighted row in place; press again to mask | yes (`reveal`) |
| action chords from list mode | Same as list mode | yes |
| `Escape`, `Left`, `Shift+Tab` | Back to list mode (drops revealed values) | no |

Reveal rules:

1. At most one row is revealed at a time.
2. A secret row shows its plaintext; a one-time-code row shows the current code and the seconds
   left in its period, refreshing when the period ends.
3. A row whose value is already on screen ignores the key — no change, no notice.
4. The value is fetched at the moment of the press, never before, and is dropped on mask,
   highlight move, leaving the screen, or window close.
5. A fetch that fails leaves the row masked and reports why on the existing notice line.
6. A value that arrives after its field has moved, been renamed or been removed is discarded.

## Detail pane mode

**Removed.** Nothing opens it; the field list covers it. The `open_detail` action no longer
exists in preferences, and a stored `open_detail` binding is ignored on load without disturbing
any other binding.

## Mouse — changed rows

- Click a row in the field list: copy that field (unchanged).
- No mouse control opens a detail pane.
