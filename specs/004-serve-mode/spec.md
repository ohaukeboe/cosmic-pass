# Feature Specification: Windowless `serve` mode

**Feature Branch**: `004-serve-mode`

**Created**: 2026-11-18

**Status**: Draft

**Input**: User description: "Add a windowless `cosmic-pass serve` subcommand that drives the
existing reducer over JSON lines on stdin/stdout, for a Quickshell front end. The draft contract
is ~/projects/dot-emacs/specs/016-proton-pass-quick-access/contracts/serve-protocol.md and the
task list is T002–T008 in ~/projects/dot-emacs/specs/016-proton-pass-quick-access/tasks.md."

## Context

The user runs a second desktop session (Mango with a Quickshell bar) where the COSMIC
layer-shell popup is the wrong look. That session draws its own quick-access popup and needs a
backend that does everything `cosmic-pass` already does — session probing, the encrypted cache,
search, usage ranking, copying with clipboard expiry, reveal, sign-in — without drawing anything.

`cosmic-pass serve` is that backend: the same reducer, fed by intents read from standard input,
reporting its state as complete snapshots on standard output. The wire format is
[`contracts/serve-protocol.md`](contracts/serve-protocol.md); it is authoritative, and the front
end's copy (dot-emacs specs/016) links to it.

## User Scenarios & Testing *(mandatory)*

The "user" of this feature is a front-end process the person's desktop shell starts; each story
names what the person sees through it.

### User Story 1 - Find and copy a password through a front end (Priority: P1)

A front end starts `cosmic-pass serve`, sends "show", then the characters the person types as
query intents, then "copy primary". The backend searches the same items the COSMIC popup would,
copies the selected item's password with the configured clipboard expiry, and reports in its
next snapshot that the popup should close.

**Why this priority**: Copying a password is almost all of the feature's use; without it nothing
else matters.

**Independent Test**: Drive the real `serve` loop with the fake backend: send `show`, a `query`,
`copy_primary`; read the snapshots; confirm the rows match the query, the clipboard received the
password, and the last snapshot has `visible: false` and no secret anywhere.

**Acceptance Scenarios**:

1. **Given** a signed-in session with cached items, **When** the backend starts, **Then** it
   writes a first snapshot before reading any intent, and a later one with rows once items load.
2. **Given** rows for a query, **When** `next`, `prev`, `page_down`, `page_up` or
   `select {index}` arrive, **Then** the next snapshot's `selected` moves exactly as it would in
   the COSMIC popup.
3. **Given** an item is selected, **When** `copy_primary` arrives, **Then** its primary secret is
   copied, the clipboard clears after the configured seconds, and the next snapshot has
   `visible: false` and contains the copied value nowhere.
4. **Given** an empty query, **When** the snapshot is written, **Then** `rows` lists the most-used
   items first, at most 50.

---

### User Story 2 - Copy, reveal or see another field (Priority: P2)

The front end sends "open fields" for the selected item. The snapshot switches to the fields
mode and lists every copyable field; non-secret values are present, secret ones are masked until
a "reveal" intent, and only one secret is ever visible at a time.

**Why this priority**: Usernames, TOTP codes and other fields are the second most common need,
and reveal is the only way to read a value without putting it on the clipboard.

**Independent Test**: Through the serve loop: `open_fields`, `reveal`, `next`, `activate`; check
each snapshot's `mode`, `fields`, `item` and that guarantees 1–3 of the contract hold.

**Acceptance Scenarios**:

1. **Given** an item selected in list mode, **When** `open_fields` arrives, **Then** the snapshot
   has `mode: "fields"`, `item` describing the item, and one `fields` entry per copyable field.
2. **Given** fields mode with a secret field selected, **When** `reveal` arrives, **Then** a
   snapshot shows `revealing: true` for that field, then one with its plaintext `value`; no other
   field has a non-null secret `value` or `totp`.
3. **Given** a revealed one-time code, **When** time passes, **Then** the backend writes a new
   snapshot each second with the current `code` and `remaining` seconds, without any intent.
4. **Given** a revealed secret, **When** the selection moves, `back`/`escape` leaves fields mode,
   `hide` arrives, or a refresh reshapes the open item, **Then** the next snapshot contains the
   secret nowhere.
5. **Given** fields mode, **When** `activate {index}` or `activate {index: null}` arrives,
   **Then** that field (or the selected one) is copied and the copied value appears in no
   snapshot.

---

### User Story 3 - Know why it is not working (Priority: P2)

Whatever is wrong — signed out, `pass-cli` missing or older than tested, session locked, data
out of date, session store unreadable, a failed copy — the snapshot says so in a field the front
end can show, and a "login" intent starts the browser sign-in with its progress in the snapshot.

**Why this priority**: A front end that shows an empty list with no reason is worse than none.

**Independent Test**: With the fake backend set to each failure, run the serve loop and snapshot
the output; each state yields a distinct `session` or text field.

**Acceptance Scenarios**:

1. **Given** each of: loading, signed out, out-of-date items, session locked, `pass-cli`
   missing, `pass-cli` older than tested, unreadable session store, failed copy, **When** the
   backend reaches it, **Then** the snapshot names it through `session`, `session_detail`,
   `stale`, `notice` or `cli_warning`.
2. **Given** `can_login` is true, **When** `login` arrives, **Then** snapshots show
   `session: "logging_in"` with the sign-in URL and progress lines as they arrive, and on failure
   or timeout return to a state with `can_login: true` and the reason.
3. **Given** `refresh` arrives, **Then** `busy.refreshing` is true until the refresh ends.

---

### User Story 4 - A robust process boundary (Priority: P3)

The front end may send malformed or newer intents, close stdin to stop the backend, or pass a
bad flag; the backend never crashes on input, never leaks a secret through its outputs, and
exits with a code the front end can act on.

**Why this priority**: The front end restarts the backend on exit; a backend that exits on a bad
line or hangs on a closed pipe defeats that.

**Independent Test**: Run the binary as a subprocess: a garbage line is followed by a normal
snapshot; closing stdin exits 0; `--clipboard-clear-secs 5` exits 2 with no stdout.

**Acceptance Scenarios**:

1. **Given** a running backend, **When** a line is not valid JSON, or names an unknown `t`,
   **Then** it is logged to stderr and skipped and later intents still work.
2. **Given** an intent with extra unknown fields, **Then** they are ignored and the intent acts.
3. **Given** a running backend, **When** stdin closes, **Then** it exits with status 0.
4. **Given** `--clipboard-clear-secs` missing or outside 10–600, **Then** it exits with status 2
   before writing anything to stdout.

### Edge Cases

- A COSMIC popup instance of `cosmic-pass` is already running: `serve` does not claim the
  single-instance name and runs beside it; both share the same cache and usage table.
- An intent arrives while a refresh or copy is in flight: it is applied at once; the snapshot
  reflects both.
- A reducer update changes nothing visible (e.g. `next` at the last row): no snapshot is written.
- `select {index}` beyond the rows, or `activate {index}` beyond the fields: ignored, as in the
  reducer today.
- `select` in fields mode: moves the list selection only, as `Msg::Select` does today; the front
  end uses `activate {index}` to act on a field by position.
- stdout is closed by the front end while stdin stays open: the backend exits rather than spin.
- Preference and key-rebinding messages have no intent and cannot be reached.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The application MUST offer a `serve` subcommand that opens no window, registers no
  single-instance name, and reads and writes no desktop configuration.
- **FR-002**: `serve` MUST require `--clipboard-clear-secs` in 10–600 and MUST exit 2 without
  any stdout output when it is missing or out of range.
- **FR-003**: `serve` MUST start the session probe, cache load and refresh as soon as it starts,
  before reading the first intent.
- **FR-004**: `serve` MUST read one JSON object per line from stdin and map each of the 16
  intents of the contract's table to exactly the reducer message the table names; no other
  message MUST be reachable from stdin.
- **FR-005**: A line that does not parse, or has an unknown `t`, MUST be logged to stderr and
  skipped; unknown fields in a known intent MUST be ignored.
- **FR-006**: `serve` MUST write one complete snapshot line at start and after every reducer
  update whose snapshot differs from the last one written, with exactly the fields of the
  contract and `v: 1`.
- **FR-007**: Snapshot `session` MUST be one of `unknown`, `checking`, `signed_in`,
  `signed_out`, `locked`, `cli_missing`, `logging_in`, `error`; `error` MUST carry its text in
  `session_detail`.
- **FR-008**: `rows` MUST hold at most 50 results in rank order, each with title, subtitle, vault
  and one of the contract's kinds.
- **FR-009**: In fields mode, `fields` MUST list each copyable field with label and secrecy;
  `value` MUST be set only for a stored non-secret value or the one revealed secret.
- **FR-010**: At most one field MUST carry a secret `value` or `totp`; leaving fields mode,
  hiding, moving the selection, or a refresh that reshapes the open item MUST clear it in the
  next snapshot.
- **FR-011**: A value that was copied MUST NOT appear in any snapshot.
- **FR-012**: While a one-time code is shown, `serve` MUST write a fresh snapshot each second.
- **FR-013**: Copies MUST go through the same clipboard helper and expiry as the COSMIC popup,
  with the expiry from `--clipboard-clear-secs`.
- **FR-014**: `serve` MUST NOT write snapshots or field values to stderr, the log, or any file;
  stderr carries logs only.
- **FR-015**: `serve` MUST NOT save preferences; usage ranking and the cache MUST be read and
  updated as the COSMIC popup does.
- **FR-016**: `serve` MUST exit 0 when stdin reaches end of file, and MUST exit when stdout
  becomes unwritable.
- **FR-017**: The README MUST document `serve`: its purpose, its flag, and a link to the contract.

### Key Entities

- **Intent**: One line from the front end, a `t` name plus at most one argument (`text` or
  `index`), mapping to one reducer message.
- **Snapshot**: The full visible state after an update: version, visibility, session status and
  detail, sign-in progress, mode, query, selection, rows, open item, fields, busy flags and the
  three text lines (`stale`, `notice`, `cli_warning`).
- **Field entry**: Label, whether it is secret, its value if shown, whether a reveal is in
  flight, and a one-time code with seconds remaining.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With 2,000 items, a query intent yields its snapshot in under 100 ms.
- **SC-002**: Every one of the 16 intents and every failure state of User Story 3 is covered by
  a test that drives the real serve loop.
- **SC-003**: Across the whole test suite, no snapshot ever contains a copied value or two
  visible secrets.
- **SC-004**: No input line, however malformed, makes the backend exit with a non-zero status.
- **SC-005**: A front end developer can build against the contract file alone, without reading
  the backend's source.

## Assumptions

- `locked` means the `pass-cli` session is locked (`pass-cli session unlock` fixes it), which is
  what sets it in the reducer. The draft contract said "keyring unavailable"; that was wrong and
  is corrected in this repository's copy. The reducer has no keyring-unavailable state: when the
  keyring cannot be read the cache is skipped and items load from `pass-cli`, so there is
  nothing to report. The front end's FR-019 "keyring unavailable" maps to `locked` only in the
  sense of a locked session; its status text (dot-emacs T023) must follow.
- The front end restarts the backend when it exits; `serve` needs no reconnection or
  persistence of its own beyond what the cache already gives.
- Snapshot size is not a concern: at most 50 rows plus one item's fields per line.
- The spec FR-019 referred to by the contract is dot-emacs specs/016 FR-019; the states it lists
  are those in User Story 3.
- The contract is versioned by `v`; adding snapshot fields does not raise it (contract
  guarantee 5).
