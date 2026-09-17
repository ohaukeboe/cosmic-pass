# Feature Specification: Proton Pass Quick Access

**Feature Branch**: `001-quick-access-launcher`

**Created**: 2026-09-17

**Status**: Draft

**Input**: User description: "Create a quick-access application (similar to the 1password quick-acces) for Proton Pass. Use the pass-cli tool for communicating with proton pass, and optimize the app for responsiveness. The app targets Cosmic-de, so build it with rust and the iced toolkit"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Find a login and copy its password (Priority: P1)

A user is working in any application and needs a password. They press a keyboard shortcut. A
small search window appears in the center of the screen with the cursor already in the search
field. They type a few characters of the site or item name. Matching items appear as they type.
They press Enter on the highlighted result. The password is copied to the clipboard and the
window closes. The user pastes the password where they need it.

**Why this priority**: This is the core reason the app exists. Search-and-copy of a password
without opening the full Proton Pass app delivers value on its own.

**Independent Test**: With a signed-in account that contains login items, press the shortcut,
type part of an item title, press Enter, and paste into a text editor. The pasted text equals
the item's password.

**Acceptance Scenarios**:

1. **Given** the user is signed in and the window is hidden, **When** the user presses the
   shortcut, **Then** the window appears with keyboard focus in the search field.
2. **Given** the window is open, **When** the user types "git", **Then** items whose title,
   username, or website contains "git" are listed, with the best match highlighted first.
3. **Given** a login item is highlighted, **When** the user presses Enter, **Then** the
   item's password is on the clipboard and the window closes.
4. **Given** the window is open, **When** the user presses Escape or the window loses focus,
   **Then** the window closes and the search text is cleared.
5. **Given** a password was copied, **When** the clipboard clear timeout expires and the
   clipboard still holds that password, **Then** the clipboard is cleared.

---

### User Story 2 - Copy other fields and one-time codes (Priority: P2)

A user finds an item and needs something other than the password: the username, a one-time
code (TOTP), the website address, a credit card number, or the content of a secure note. They
use a keyboard shortcut or open an action list on the highlighted item to pick which field to
copy.

**Why this priority**: Two-factor codes and usernames are needed in almost every sign-in.
It extends the P1 flow and is useless without search.

**Independent Test**: Highlight a login that has a TOTP secret, press the copy-code shortcut,
and paste. The pasted value matches the current code shown by Proton Pass.

**Acceptance Scenarios**:

1. **Given** a login is highlighted, **When** the user presses the copy-username shortcut,
   **Then** the username is copied and the window closes.
2. **Given** a login with a TOTP secret is highlighted, **When** the user presses the
   copy-code shortcut, **Then** the current code is copied and the window closes.
3. **Given** an item is highlighted, **When** the user opens the action list, **Then** all
   copyable fields for that item type are listed with their shortcuts, and secret values are
   masked.
4. **Given** a login without a TOTP secret is highlighted, **When** the user presses the
   copy-code shortcut, **Then** a short inline message says the item has no one-time code and
   the clipboard is unchanged.

---

### User Story 3 - View item details without leaving the keyboard (Priority: P3)

A user wants to check which account an item belongs to before copying. They press a key to
open a detail pane for the highlighted item. The pane shows the item's title, vault, username,
websites, and a masked password with an option to reveal it. The current TOTP code and its
remaining seconds are shown when present.

**Why this priority**: Helps users with many similar items pick the right one. The core
copy flow works without it.

**Independent Test**: Highlight an item, open the detail pane, and verify all listed fields
match the item in Proton Pass. The password stays masked until the user reveals it.

**Acceptance Scenarios**:

1. **Given** an item is highlighted, **When** the user opens the detail pane, **Then** the
   item's non-secret fields are visible and secret fields are masked.
2. **Given** the detail pane is open, **When** the user presses the reveal shortcut, **Then**
   the password is shown in plain text until the pane or window closes.
3. **Given** a TOTP code is shown, **When** its period ends, **Then** the displayed code and
   countdown update without user action.

---

### User Story 4 - Recover from a signed-out or unavailable state (Priority: P2)

A user opens the window but their Proton Pass session has expired, or the Proton Pass
command-line tool is missing. The window tells them what is wrong and how to fix it, instead
of showing an empty list.

**Why this priority**: Without clear status, the P1 flow silently fails and the user cannot
tell why.

**Independent Test**: Sign out of Proton Pass, open the window, and verify a sign-in message
with an action to start sign-in is shown.

**Acceptance Scenarios**:

1. **Given** the user is signed out, **When** the window opens, **Then** it shows a
   "Not signed in" message and an action that starts the Proton Pass sign-in flow.
2. **Given** the Proton Pass command-line tool is not installed, **When** the window opens,
   **Then** it shows a message naming the missing tool and where to get it.
3. **Given** a network error occurs during refresh, **When** cached results exist, **Then**
   the cached results stay usable and a small indicator shows the data may be stale.

---

### Edge Cases

- Search returns no matches: show a "No items found" message; Enter does nothing.
- User types while the item list is still loading: input is never blocked; results update
  when data arrives, and the typed query is kept.
- Two items have the same title in different vaults: each result shows its vault name.
- Item was deleted or changed in Proton Pass since the last refresh: copying fetches the
  current value; if the item no longer exists, show "Item no longer exists" and refresh.
- Very large accounts (5,000+ items): search stays within the responsiveness targets.
- The shortcut is pressed while the window is already open: the window closes (toggle).
- Item has multiple websites or multiple TOTP fields: all are listed in the action list.
- Trashed items are never shown in results.
- Retrieving a secret takes longer than expected: show a busy indicator on the item; the
  user can cancel with Escape without the value being copied later.
- The clipboard contents were changed by the user before the clear timeout: the app does not
  clear the clipboard.

## Requirements *(mandatory)*

### Functional Requirements

**Invocation and window**

- **FR-001**: Users MUST be able to open and close the quick-access window with a single
  system-wide keyboard shortcut.
- **FR-002**: The window MUST open centered on the active display, above other windows, with
  keyboard focus in the search field.
- **FR-003**: The window MUST close on Escape, on loss of focus, and after a successful copy.
  Closing MUST clear the search text and any revealed secrets.
- **FR-004**: All actions MUST be reachable by keyboard alone. Mouse use MUST also work for
  selecting and copying.

**Search**

- **FR-005**: The system MUST search across all vaults the user can access, matching against
  item title, username/email, website addresses, and vault name.
- **FR-006**: Search MUST be case-insensitive and tolerant of partial and out-of-order
  fragments (fuzzy matching), ranking title matches above other field matches.
- **FR-007**: Results MUST update on every keystroke without waiting for a submit action.
- **FR-008**: Each result MUST show the item title, item type icon, a secondary line
  (username, card holder, or note preview without secret content), and vault name.
- **FR-009**: With an empty query, the system MUST show recently used items first.
- **FR-010**: Trashed items MUST be excluded from results.

**Copy actions**

- **FR-011**: Enter MUST copy the item's primary secret: the password for logins, the card
  number for credit cards, the note content for notes, and the first custom secret for other
  types.
- **FR-012**: Users MUST be able to copy the username, the current one-time code, and the
  primary website of a login with dedicated keyboard shortcuts.
- **FR-013**: Users MUST be able to open an action list for the highlighted item showing
  every copyable field and its shortcut.
- **FR-014**: The system MUST fetch secret values only when the user requests a copy or
  reveal, and MUST NOT hold them in memory after the action completes.
- **FR-015**: Copied secrets MUST be removed from the clipboard after a timeout (default
  90 seconds, user configurable), unless the clipboard content has changed since the copy.
- **FR-016**: Copied secrets MUST be marked so clipboard managers that honor the
  "sensitive content" hint do not store them in history.
- **FR-017**: The system MUST deliver values to other applications only through the
  clipboard. Typing or filling credentials into the focused application (auto-type) is out
  of scope for v1.

**Details**

- **FR-018**: Users MUST be able to open a detail pane showing the highlighted item's fields
  with secret values masked by default and revealable on request.
- **FR-019**: One-time codes MUST display a countdown and refresh automatically when they
  expire.

**Session and status**

- **FR-020**: The system MUST detect and clearly report these states: signed out, Proton Pass
  command-line tool missing, network unavailable, and unexpected tool errors.
- **FR-021**: When signed out, the system MUST offer an action that starts the Proton Pass
  sign-in flow.
- **FR-022**: The system MUST refresh the item list in the background without blocking
  input, and MUST indicate when displayed data may be stale.

**Responsiveness and data handling**

- **FR-023**: The item list (titles and non-secret metadata) MUST be available immediately
  when the window opens, without waiting for Proton Pass to respond.
- **FR-024**: Non-secret item metadata used for search MUST be kept in memory and also
  persisted to disk, encrypted with a key held in the system keyring, so results appear
  instantly after login or reboot. The cache MUST be refreshed from Proton Pass in the
  background on startup and on each window opening that follows a refresh interval.
- **FR-024a**: If the keyring is unavailable or locked, the system MUST NOT write the cache
  to disk and MUST fall back to in-memory metadata only.
- **FR-024b**: Signing out of Proton Pass or switching accounts MUST delete the on-disk cache.
- **FR-025**: The system MUST never write secret values (passwords, codes, card numbers, note
  contents) to disk or logs.
- **FR-026**: Recently used item history MUST store only item identifiers, never secret
  values.

**Configuration**

- **FR-027**: Users MUST be able to configure the clipboard clear timeout and the per-action
  keyboard shortcuts inside the window.

### Key Entities

- **Vault**: A named container of items the user can access. Attributes: identifier, name.
- **Item**: An entry in a vault. Attributes: identifier, vault, type (login, note, credit
  card, identity, alias, SSH key, Wi-Fi, custom), title, non-secret summary fields (username,
  websites), flags (has one-time code), state (active or trashed), last-modified time.
- **Secret field**: A sensitive value belonging to an item (password, one-time code secret,
  card number, note content, custom hidden field). Fetched on demand only.
- **Usage record**: Item identifier and last-used time, used to rank recent items.
- **Session state**: Whether the user is signed in, and whether the Proton Pass tool is
  available and reachable.
- **Preferences**: Clipboard clear timeout and action shortcuts.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The window is visible and accepts typing within 100 ms of pressing the shortcut
  in 95% of openings once the app has started.
- **SC-002**: Search results update within 50 ms of each keystroke for accounts with up to
  5,000 items.
- **SC-003**: A user can go from shortcut press to a copied password in under 3 seconds for a
  known item.
- **SC-004**: Typing is never dropped or delayed while data refreshes or secrets load (0
  dropped keystrokes in testing).
- **SC-005**: Copied secrets are cleared from the clipboard within 1 second of the configured
  timeout in 100% of cases where the clipboard still holds them.
- **SC-006**: No secret value appears in any file, log, or crash report written by the app
  (verified by scanning all written files during testing).
- **SC-007**: In a signed-out or tool-missing state, 100% of window openings show an
  actionable status message instead of an empty list.
- **SC-008**: 90% of test users complete the "copy password for a named site" task on first
  attempt without instructions.

## Assumptions

- **Platform constraint (from request)**: The app targets the COSMIC desktop on Linux and is
  built in Rust with the iced toolkit, matching COSMIC's native look. Other desktops are out
  of scope for v1.
- **Dependency (from request)**: All communication with Proton Pass goes through the official
  Proton Pass command-line tool (`pass-cli`), which the user installs and signs in to. The app
  does not talk to Proton servers directly and does not store Proton credentials.
- The app runs as a lightweight background process started with the user session, so the
  window can appear instantly; the system shortcut only toggles the window.
- The system-wide shortcut is registered through the desktop's standard shortcut settings.
- The app is read-only in v1: creating, editing, deleting, or sharing items is out of scope.
- Auto-type into other applications is deferred to a later version; it is hard to do
  reliably and securely on Wayland.
- A system keyring (Secret Service) is available in the user session for the cache
  encryption key.
- Password generation, passkeys, attachments, and alias management are out of scope for v1.
- One Proton account at a time (whichever account the command-line tool is signed in to).
- Clipboard clear timeout default is 90 seconds, a common password-manager default.
- Search matching and ranking happen locally on non-secret metadata; secret values are fetched
  from Proton Pass only at the moment of use.
- Users have Proton Pass items already; import and onboarding are out of scope.
