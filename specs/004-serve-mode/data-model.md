# Data Model: Windowless `serve` mode

The wire shapes are defined in [`contracts/serve-protocol.md`](contracts/serve-protocol.md); this
file says where each value comes from in the reducer's `Model` and what rules hold.

## Intent

One line from stdin. Tagged by `t`; unknown fields ignored; unknown `t` or a parse failure is
logged (without the line's content) and skipped.

| `t`            | Argument               | `Msg`                    |
|----------------|------------------------|--------------------------|
| `show`         |                        | `Show`                   |
| `hide`         |                        | `Hide`                   |
| `query`        | `text: String`         | `QueryChanged(text)`     |
| `next`         |                        | `SelectNext`             |
| `prev`         |                        | `SelectPrev`             |
| `page_down`    |                        | `PageDown`               |
| `page_up`      |                        | `PageUp`                 |
| `select`       | `index: usize`         | `Select(index)`          |
| `copy_primary` |                        | `CopyPrimary`            |
| `open_fields`  |                        | `OpenActions`            |
| `activate`     | `index: Option<usize>` | `ActivateAction(index)`  |
| `reveal`       |                        | `ToggleReveal`           |
| `back`         |                        | `Back`                   |
| `escape`       |                        | `Escape`                 |
| `refresh`      |                        | `RefreshRequested`       |
| `login`        |                        | `StartLogin`             |

`Select` moves the list selection in list mode, and the field highlight in fields mode, dropping
any reveal as the arrows do. The fields-mode half is new: a pointer needs it to reveal one row
(`select` then `reveal`).

## Snapshot

Projection `Snapshot::of(&Model, now)`. Pure; no `Debug`.

| Field            | Source                                                            |
|------------------|-------------------------------------------------------------------|
| `v`              | constant `1`                                                      |
| `visible`        | `view.visible`                                                    |
| `session`        | `session`, by the table below                                     |
| `session_detail` | the text of `SessionState::Error`, else `null`                    |
| `can_login`      | `can_start_login()`                                               |
| `login`          | `{url: login_url, lines: login_lines}` while `LoggingIn`          |
| `mode`           | `"list"` for `Mode::List`, `"fields"` for `Mode::Actions`         |
| `query`          | `view.query`                                                      |
| `selected`       | `view.selected` in list mode, `Mode::Actions.selected` in fields  |
| `rows`           | `view.results` → `data.items[row.item]`, in order                 |
| `item`           | `target_item()` in fields mode, else `null`                       |
| `fields`         | `actions()` in fields mode, else `[]`                             |
| `busy`           | `data.refreshing`, `view.pending.is_some()`, `view.copying`       |
| `stale`          | `stale_notice()`                                                  |
| `notice`         | `view.notice.text`                                                |
| `cli_warning`    | `cli_warning`                                                     |

`Mode::Preferences` cannot be entered from `serve` (no intent reaches it); if it ever were, it
projects as `"list"`.

### Session mapping

| `SessionState` | `session`     |
|----------------|---------------|
| `Unknown`      | `unknown`     |
| `Checking`     | `checking`    |
| `SignedIn(_)`  | `signed_in`   |
| `SignedOut`    | `signed_out`  |
| `Locked`       | `locked`      |
| `CliMissing`   | `cli_missing` |
| `LoggingIn`    | `logging_in`  |
| `Error(text)`  | `error`       |

### Row and item

`{title, subtitle, vault, kind}` from `ItemSummary.title`, `.subtitle`, `.vault_name`, and
`.kind` mapped per research R10. At most `prefs.max_results` rows; `serve` sets it to `50`.

### Field entry

From each `ActionEntry` (research R11):

| Field       | Rule                                                                    |
|-------------|-------------------------------------------------------------------------|
| `label`     | `entry.label`                                                           |
| `secret`    | `entry.source.is_secret()`                                              |
| `value`     | plain field: `FieldRef.value`; secret field: `revealed_value(index)`    |
| `revealing` | field: `revealing_field(index)`; code: `revealing_totp()`               |
| `totp`      | `{code, remaining(now)}` if `revealed_totp()` is this entry's field     |

## Model change

`Model` gains `login_lines: Vec<String>` (research R8):

- `StartLogin` → cleared.
- `LoginLine(line)` → pushed.
- `LoginFinished(_)` → cleared (with `login_url`).

## Invariants (contract guarantees)

1. At most one entry has a secret `value` or a `totp`: `Revealed` is one slot
   (`src/core/state.rs`, `enum Revealed`), and `revealed_value` re-checks its target.
2. Leaving fields mode, `Hide`, moving the selection, or a reshaping refresh clears it: already
   true of `Revealed` in the reducer; `serve` adds no state that could hold it.
3. No copied value appears: copies flow through `Effect::FetchAndCopy`/`Copy`, whose values never
   enter `Model` fields the snapshot reads.
4. Snapshots go to stdout only; the comparison copy (research R6) is a `SecretString`.
