# Data Model: Proton Pass Quick Access

**Feature**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md) | **Date**: 2026-09-17

Types below are conceptual Rust shapes. Secret-bearing types never implement `Serialize`,
`Debug` with content, or `Clone` into long-lived state.

## Identifiers

| Type | Shape | Notes |
|------|-------|-------|
| `ShareId` | newtype `String` | Vault share ID from `pass-cli`. May change across sessions. |
| `ItemId` | newtype `String` | Unique only within a share. |
| `ItemKey` | `(ShareId, ItemId)` | Global item identity. Used for usage records and lookups. |
| `AccountId` | newtype `String` | From `pass-cli info`. Scopes the cache. |

## Vault

| Field | Type | Rules |
|-------|------|-------|
| `share_id` | `ShareId` | Required, non-empty. |
| `name` | `String` | Required. Shown on every result row. |

## ItemKind

`Login | Note | CreditCard | Identity | Alias | SshKey | Wifi | Custom | Unknown(String)`

Unknown kinds from newer `pass-cli` versions are kept and shown with a generic icon.

## ItemSummary (non-secret; searchable; cached)

| Field | Type | Rules |
|-------|------|-------|
| `key` | `ItemKey` | Required. |
| `vault_name` | `String` | Copied from `Vault` for display and search. |
| `kind` | `ItemKind` | Required. |
| `title` | `String` | Required; empty titles shown as "(untitled)". |
| `username` | `Option<String>` | Login username (empty strings become `None`). |
| `email` | `Option<String>` | Login or identity email. |
| `subtitle` | `Option<String>` | Username (else email) for logins, card holder for cards, full name for identities, SSID for Wi-Fi. `None` for notes, aliases, SSH keys, and custom items: their only distinguishing content is secret, and a note preview would put the note body on screen, so those rows show the title alone (decision 2026-09-18, FR-008). Never note content or any secret. |
| `urls` | `Vec<String>` | Login websites. Search uses the host part. |
| `totp_fields` | `Vec<String>` | TOTP field names (`totp_uri` for the login code, custom TOTP names). `has_totp()` is `!totp_fields.is_empty()`. |
| `fields` | `Vec<FieldRef>` | Copyable standard and custom fields in display order, excluding TOTP fields. Every login website becomes one field: `url` ("Website") for the first, then `url2`, `url3`, ... ("Website 2", ...). |
| `modified_at` | `i64` (unix s) | For tie-breaking and change detection. |

Validation:

- Items with state `trashed` are dropped during parsing (FR-010).
- The parser MUST NOT copy any of: password, TOTP secret/URI, card number, CVV, PIN,
  note body, hidden custom field values, SSH private key, Wi-Fi password.

## FieldRef

| Field | Type | Rules |
|-------|------|-------|
| `name` | `String` | `pass-cli` field name (`username`, `password`, `totp`, custom name, `Section.field`), or a synthetic name for a field whose value is already stored (`url2`, `url3`, ... for the second and later websites). A custom field may carry the same name; lookups take the first match, and every website entry stores its own value, so each row still copies its own URL. |
| `label` | `String` | Human label for the action list. |
| `secret` | `bool` | Masked in UI; triggers clipboard timeout on copy. |
| `value` | `Option<String>` | Only for non-secret standard fields (username, email, every URL, card holder, expiry, SSID, public key): copied without calling `pass-cli`. `None` for secrets and for custom `Text` fields (fetched on demand, no timeout). |

## CopyAction

Derived per item kind (FR-011–FR-013):

| Kind | Primary (Enter) | Other actions |
|------|-----------------|---------------|
| Login | `password` | `username`, `email`, `totp`, every website (`url`, `url2`, ...), each custom field |
| CreditCard | card `number` | holder name, expiry, `cvv`, custom fields |
| Note | `note` | custom fields |
| Alias | alias email | `note` |
| Identity, SshKey, Wifi, Custom | first secret custom/standard field; else first field | all fields |

`CopySource = Field(FieldRef) | Totp { field: String }`. A `Field` with a stored `value`
needs no `pass-cli` call. Action-list entries are `ActionEntry { source, label, shortcut }`,
primary first, then fields in order, then TOTP fields. A keyboard shortcut is bound to at
most one entry: copy-website to the first website (`url`), copy-username to the first
`username` field, else the first `email` field, copy-one-time-code to the first TOTP field.

## SecretValue (never cached, never logged)

`secrecy::SecretString`. Lives only between fetch and hand-off to the clipboard helper or
the detail-pane reveal. Revealed values are dropped when the pane or window closes (FR-003).

## TotpDisplay

| Field | Type | Rules |
|-------|------|-------|
| `field` | `String` | TOTP field name. |
| `code` | `SecretString` | From `pass-cli item totp`. |
| `period` | `u32` | 30 (assumed; `pass-cli` returns none). |
| `valid_until` | `i64` | Next multiple of `period`. Refetch when now ≥ `valid_until`. |

## UsageRecord (cached)

| Field | Type | Rules |
|-------|------|-------|
| `key` | `ItemKey` | Required. |
| `last_used` | `i64` | Updated on every successful copy. |
| `count` | `u32` | Saturating increment. |

Max 200 records; oldest evicted. Records whose item disappears are pruned on refresh.
Contains no values (FR-026).

## SessionState

```text
Unknown ──probe──▶ Checking
Checking ─ok────▶ SignedIn { account: AccountId }
Checking ─err───▶ SignedOut | Locked | CliMissing | Error { message }
SignedIn ─refresh error SignedOut─▶ SignedOut       (delete cache: FR-024b)
SignedIn ─info returns other account─▶ SignedIn{new} (delete cache, refetch)
SignedOut ─user starts login─▶ LoggingIn
LoggingIn ─login exits 0─▶ Checking
LoggingIn ─login fails─▶ Error { message }
any ─network error─▶ state unchanged, data.stale = true
```

## DataState

| Field | Type | Rules |
|-------|------|-------|
| `vaults` | `Vec<Vault>` | In `pass-cli` order. |
| `items` | `Vec<ItemSummary>` | All active items across vaults. |
| `fetched_at` | `Option<i64>` | Last successful full refresh. |
| `stale` | `bool` | True if last refresh failed or data came from cache and refresh is pending. |
| `refreshing` | `bool` | A refresh task is in flight. Only one at a time. |
| `source` | `Memory \| DiskCache` | Where the current items came from. |
| `last_error` | `Option<RefreshError>` | Why the last refresh or session probe failed: `Unreachable` for `PassError::Network`/`Timeout`, else `Other`. Cleared on the next successful load and on sign-out. Chooses the status line (FR-020): "Can't reach Proton Pass — showing saved items." vs the generic "Data may be out of date." |
| `cached_at` | `Option<i64>` | `fetched_at` recorded in the loaded cache file. |

Refresh replaces `items` atomically only after every vault listing succeeds. A partial
failure keeps the old list and sets `stale`.

## ViewState (window UI; reset on hide)

| Field | Type | Rules |
|-------|------|-------|
| `visible` | `bool` | |
| `query` | `String` | Cleared on hide (FR-003). |
| `results` | `Vec<ResultRow>` | Max 50. Recomputed on query or data change. |
| `selected` | `usize` | Clamped to `results.len()`. Reset to 0 on query change. |
| `mode` | `List \| Actions { key, selected } \| Detail { key } \| Preferences { rebinding }` | Escape cancels a pending fetch or rebinding first, then goes back one level; from `List` it hides. |
| `pending` | `Option<PendingFetch>` | `{ key, cancel: CancellationToken, secret }`. Escape cancels. |
| `revealed` | `Option<SecretString>` | Detail pane only. Dropped on mode change or hide. |
| `totp` | `Option<TotpDisplay>` | Detail pane only. |
| `notice` | `Option<Notice>` | Inline message, e.g. "No one-time code for this item". Auto-clears after 3 s. |

`ResultRow = { item: usize, score: u32, title_indices: Vec<u32> }` (`item` indexes
`DataState.items`).

## ClipboardJob

| Field | Type | Rules |
|-------|------|-------|
| `helper` | child process handle | `cosmic-pass clipboard-serve`. |
| `expires_at` | `Instant` | now + `clipboard_clear_secs`. |
| `secret` | `bool` | Non-secret copies (URL, username) do not start a timeout. |

Only one job at a time. A new copy kills the previous helper first.
Helper exits early → ownership lost → job ends, nothing to clear.
Timer fires → kill helper → selection cleared by compositor.

## Preferences (cosmic-config)

See [contracts/config.md](./contracts/config.md).

## CacheFile (encrypted on disk)

| Field | Type |
|-------|------|
| `format_version` | `u16` (= 1) |
| `account` | `AccountId` |
| `fetched_at` | `i64` |
| `vaults` | `Vec<Vault>` |
| `items` | `Vec<ItemSummary>` |
| `usage` | `Vec<UsageRecord>` |

Envelope and rules: [contracts/cache-format.md](./contracts/cache-format.md).
