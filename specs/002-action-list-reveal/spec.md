# Feature Specification: One surface for item fields

**Feature Branch**: `002-action-list-reveal`

**Created**: 2026-09-19

**Status**: Draft

**Input**: User description: "The custom text fields which it fails to show the content for, it should just not show at all. Also, it does not need to have both the info screen (behind ctrl+i) and the action list behind Tab. Remove the info screen and make ctrl+r work in the action list making it reveal the field currently selected"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Reveal a secret from the action list (Priority: P1)

A user finds an item, opens its field list, moves the highlight to the field they want to
check, and reveals that field's value in place — without copying it and without opening a
second screen. Pressing reveal again hides it.

**Why this priority**: Reveal is the only capability the removed info screen offered that the
field list does not. Without it, removing the info screen loses functionality.

**Independent Test**: Open an item's field list, highlight a masked field, press the reveal
shortcut, and confirm the value appears on that row and only that row. Press reveal again and
confirm it is masked again.

**Acceptance Scenarios**:

1. **Given** the field list is open with a masked field highlighted, **When** the user presses
   the reveal shortcut, **Then** that field's value is shown in place of the mask and every
   other field stays masked.
2. **Given** a field is revealed, **When** the user presses the reveal shortcut again on the
   same field, **Then** the value is masked again.
3. **Given** a field is revealed, **When** the user moves the highlight to another field,
   **Then** the previously revealed field is masked again and the newly highlighted field is
   not revealed until the user asks.
4. **Given** the highlighted row is a one-time code, **When** the user presses the reveal
   shortcut, **Then** the current code is shown with the seconds remaining in its period, and
   the code and countdown refresh when the period ends.
5. **Given** the highlighted row is a field that is not secret, **When** the user presses the
   reveal shortcut, **Then** nothing changes and no error is reported (its value is already
   shown).
6. **Given** a field is revealed, **When** the window closes or the user returns to the result
   list, **Then** the revealed value is discarded and the field is masked on the next opening.

---

### User Story 2 - One screen for an item's fields (Priority: P2)

A user who wants to see everything about an item opens the field list with Tab. There is no
second, near-duplicate screen to learn, and no shortcut that leads to one.

**Why this priority**: Removing the duplicate screen is the user's stated goal, but it only
becomes safe once reveal exists in the field list (Story 1).

**Independent Test**: Press the former info-screen shortcut from the result list and confirm
no separate screen opens; confirm the field list still reaches every field the info screen
showed.

**Acceptance Scenarios**:

1. **Given** an item is highlighted in the result list, **When** the user presses the shortcut
   that previously opened the info screen, **Then** no info screen opens.
2. **Given** the user opens the preferences screen, **When** they review the list of bindable
   actions, **Then** the info-screen action is absent and no key chord remains assigned to it.
3. **Given** a saved preference file that still binds the removed info-screen action, **When**
   the app starts, **Then** it starts normally, ignores that binding, and keeps every other
   binding.
4. **Given** the field list is open, **When** the user compares it with what the info screen
   used to show, **Then** the item title, type, vault, every listed field, and the one-time
   code are all reachable from the field list.

---

### User Story 3 - No empty field rows (Priority: P3)

A user opens an item that has user-defined text fields whose content the app cannot show.
Those rows are absent instead of being listed with an empty-value placeholder.

**Why this priority**: A cosmetic and trust problem — a row that promises a value and shows a
dash reads as a broken field — but it does not block the other two stories.

**Independent Test**: Open an item carrying a user-defined text field, and confirm that field
is not listed while its secret and built-in siblings still are.

**Acceptance Scenarios**:

1. **Given** an item with a user-defined text field, **When** the user opens its field list,
   **Then** that field is not listed.
2. **Given** an item whose only user-defined fields are text fields, **When** the user opens
   its field list, **Then** the list shows the item's remaining fields and is not empty for
   items that have other fields.
3. **Given** an item with a user-defined hidden (secret) field or a user-defined one-time-code
   field, **When** the user opens its field list, **Then** those fields are still listed.
4. **Given** an item whose fields are all user-defined text fields, **When** the user opens its
   field list, **Then** the screen states the item has no copyable fields rather than showing
   an empty list.

---

### Edge Cases

- Reveal is pressed while the value is still being fetched: the row shows that a fetch is in
  flight and the value replaces it when it arrives.
- The fetch for a reveal fails (tool error, signed out, offline): the row stays masked and the
  existing inline status line reports why.
- A background refresh renames, reorders, or deletes the revealed field while its value is in
  flight: the arriving value MUST NOT be drawn under a different field.
- The item disappears from the listing while its field list is open: the existing
  "item no longer exists" behavior is unchanged.
- An item ends up with no listable fields once user-defined text fields are dropped: the field
  list says so instead of rendering an empty list.
- The same user-defined name exists both as a text field and as a hidden field: only the hidden
  one is listed.

## Requirements *(mandatory)*

### Functional Requirements

**Reveal in the field list**

- **FR-101**: Users MUST be able to reveal the value of the highlighted field from the field
  list with a keyboard shortcut (default Ctrl+R), and hide it again with the same shortcut.
- **FR-102**: At most one field MUST be revealed at a time. Moving the highlight MUST mask any
  revealed field.
- **FR-103**: Revealing MUST fetch the value only at the moment it is asked for, MUST NOT write
  it to disk or logs, and MUST discard it when the field is masked, the highlight moves, the
  screen is left, or the window closes (extends FR-014, FR-025).
- **FR-104**: Revealing a one-time-code row MUST show the current code with the seconds
  remaining in its period, refreshing when the period ends (carries FR-019 over from the
  removed screen).
- **FR-105**: The reveal shortcut MUST do nothing on a row whose value is already displayed,
  and MUST NOT report an error.
- **FR-106**: A revealed value that arrives after the field it was requested for has moved,
  been renamed, or been removed MUST be discarded rather than displayed.
- **FR-107**: The field list MUST state which shortcut reveals the highlighted field.

**Removing the info screen**

- **FR-108**: The info screen MUST be removed: no shortcut, no on-screen control, and no
  navigation path may open it.
- **FR-109**: The bindable "show details" action MUST be removed from preferences, and its
  former default chord (Ctrl+I) MUST become unbound.
- **FR-110**: Preference files that still bind the removed action MUST load without error; the
  stale binding MUST be ignored and all other bindings preserved.
- **FR-111**: Everything the info screen showed — item title, item type, vault, each field with
  its value or mask, and the one-time code — MUST remain reachable from the field list.
- **FR-112**: FR-018 of feature 001 is superseded: masked-by-default fields with reveal on
  request are now a property of the field list, not of a separate pane.

**Fields with no value to show**

- **FR-113**: User-defined text fields, whose content the app cannot show, MUST NOT be listed
  anywhere in the interface and MUST NOT be offered as copy targets or shortcut targets.
- **FR-114**: User-defined hidden (secret) fields and user-defined one-time-code fields MUST
  still be listed.
- **FR-115**: Built-in fields whose value is fetched on demand MUST still be listed and MUST
  still be copyable; their unshown value MUST be rendered with the existing placeholder.
- **FR-116**: Dropping these fields MUST NOT change which field Enter copies, and MUST NOT
  change which field the dedicated username, website, or one-time-code shortcuts copy.
- **FR-117**: When an item has no listable fields, the field list MUST say so instead of
  showing an empty list.

### Key Entities

- **Field row**: One line of the field list — a label, the value or a mask, and the shortcuts
  that act on it. Carries whether its value is secret, already known, or fetched on demand.
- **Revealed field**: The single field whose plaintext is currently on screen, pinned to the
  item and position it was requested for so a late value cannot land on another field.
- **User-defined text field**: A non-secret field a user added to an item in Proton Pass, whose
  content the app cannot show.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user can read any one secret of a highlighted item in at most two keystrokes
  after the item is highlighted (open the field list, reveal).
- **SC-002**: There is exactly one screen in the app that lists an item's fields.
- **SC-003**: No field row in the app displays an empty-value placeholder for a user-defined
  text field; 100% of such fields are absent from the interface.
- **SC-004**: Every field and code the removed screen displayed is still reachable; zero
  capabilities are lost by the removal.
- **SC-005**: Existing users' saved shortcut preferences survive the upgrade: 100% of bindings
  other than the removed action are preserved, with no start-up error.
- **SC-006**: A revealed secret is gone from the interface within one action (mask, move,
  leave, or close) and is never written to disk.

## Assumptions

- "Info screen" is the detail screen behind Ctrl+I, and "action list" is the field list behind
  Tab; the field list is the surface that stays.
- Reveal follows the highlight rather than cycling through secrets in order, since the field
  list already has a highlighted row — this replaces the removed screen's cycle-through-secrets
  behavior.
- Revealing a one-time code shows the countdown (user-confirmed), so the countdown requirement
  of feature 001 survives the removal.
- Only user-defined text fields are dropped (user-confirmed). Built-in fields that are fetched
  on demand keep their placeholder row because copying them works.
- Hiding these fields loses nothing, because the app can neither show nor copy their content
  today.
- The removal is a breaking change to a user-visible shortcut; an unbound Ctrl+I is acceptable
  and no migration prompt is needed.
- Mouse interaction is unchanged: clicking a row still activates its copy action.
