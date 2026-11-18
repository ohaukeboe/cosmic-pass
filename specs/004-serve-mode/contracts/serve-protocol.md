# Contract: `cosmic-pass serve`

The interface between a front end (the Quickshell Rail popup in the user's
`dot-emacs` repository, specs/016-proton-pass-quick-access) and the backend
(`cosmic-pass serve`). This copy is authoritative; the front end's copy is a
link plus the list of fields it reads.

## Invocation

```text
cosmic-pass serve --clipboard-clear-secs <10..600>
```

- Opens no window and registers no single-instance name.
- Does not read or write cosmic-config.
- stdin: intents. stdout: snapshots. stderr: logs, never a field value.
- Sends `Startup` to its reducer at once, so the session is probed, the cache
  loaded and a refresh started before the first intent.
- Exits 0 when stdin closes. An invalid flag exits 2 before any output.

## Framing

One JSON object per line, UTF-8, in both directions. A line the backend
cannot parse is logged and skipped; it does not exit.

## Intents (shell to backend)

Each is `{"t": "<name>", ...}` and maps to one existing `Msg`.

| `t`             | Other fields     | `Msg`              |
|-----------------|------------------|--------------------|
| `show`          |                  | `Show`             |
| `hide`          |                  | `Hide`             |
| `query`         | `text: string`   | `QueryChanged`     |
| `next`          |                  | `SelectNext`       |
| `prev`          |                  | `SelectPrev`       |
| `page_down`     |                  | `PageDown`         |
| `page_up`       |                  | `PageUp`           |
| `select`        | `index: number`  | `Select`           |
| `copy_primary`  |                  | `CopyPrimary`      |
| `open_fields`   |                  | `OpenActions`      |
| `activate`      | `index: number \ | null`              |
| `reveal`        |                  | `ToggleReveal`     |
| `back`          |                  | `Back`             |
| `escape`        |                  | `Escape`           |
| `refresh`       |                  | `RefreshRequested` |
| `login`         |                  | `StartLogin`       |
| `copy_username` |                  | `CopyUsername`     |
| `copy_totp`     |                  | `CopyTotp`         |
| `copy_url`      |                  | `CopyUrl`          |
| `retry`         |                  | `Startup`          |

The preference and key-rebinding messages are not reachable.
`copy_username`, `copy_totp` and `copy_url` are the COSMIC popup's `Ctrl+U`,
`Ctrl+O` and `Ctrl+L`. `retry` is its "Try again": the front end offers it
for `locked`, `cli_missing` and `error` only.

## Snapshots (backend to shell)

The backend writes one snapshot after every reducer update that changes what
is below, and one at start. Each is complete; the shell keeps only the last.

```json
{
  "v": 1,
  "visible": true,
  "session": "signed_in",
  "session_detail": null,
  "can_login": false,
  "login": null,
  "mode": "list",
  "query": "git",
  "selected": 0,
  "rows": [
    {"title": "GitHub", "subtitle": "octocat", "vault": "Personal", "kind": "login"}
  ],
  "item": null,
  "fields": [],
  "busy": {"refreshing": false, "fetching": false, "copying": false},
  "stale": null,
  "notice": null,
  "cli_warning": null
}
```

- `v`: protocol version, `1`. The shell shows "backend and shell disagree"
  and nothing else when it reads another number.
- `visible`: the reducer's window state. It turns `false` after a successful
  copy and after `escape` from the list; the shell then closes the popup.
- `session`: `unknown`, `checking`, `signed_in`, `signed_out`, `locked`
  (the `pass-cli` session is locked; `pass-cli session unlock` fixes it),
  `cli_missing`, `logging_in`, `error`.
  `session_detail` is the text to show for `error`, which covers the
  unreadable session store and names its fix.
- `can_login`: whether the sign-in action is offered.
- `login`: `null`, or `{"url": string | null, "lines": [string]}` while
  `session` is `logging_in`.
- `mode`: `list` or `fields`.
- `query`: the query `rows` were computed for.
- `rows`: at most 50, best first. `kind` is one of
  `login`, `card`, `note`, `identity`, `alias`, `custom`, `other`.
- `item`: in `fields` mode, `{"title", "subtitle", "vault", "kind"}` of the
  open item; `null` otherwise.
- `fields`: in `fields` mode, one entry per copyable field:

  ```json
  {"label": "Password", "secret": true, "value": null,
   "revealing": false, "totp": null}
  ```

  `value` is the stored value of a non-secret field, the plaintext of the one
  revealed secret, or `null`. `revealing` is true while its fetch is in
  flight. `totp` is `null` or `{"code": string, "remaining": number}` for a
  revealed one-time code; the backend sends a new snapshot each second while
  one is shown.
- `selected`: index into `rows` in `list` mode, into `fields` in `fields`
  mode. The `select` intent sets it in either mode; an index out of range is
  ignored.
- `stale`, `notice`, `cli_warning`: a line of text each, or `null`. `stale`
  is the out-of-date line, `notice` a passing message (copied, copy failed,
  item gone), `cli_warning` the version warning.

## Guarantees the shell relies on

1. At most one entry of `fields` has a non-null secret `value` or `totp`.
2. Leaving `fields` mode, hiding, or moving the selection clears it in the
   next snapshot.
3. A copied value is never in any snapshot.
4. No snapshot is written to a file or to the log by the backend.
5. Unknown fields in an intent are ignored; unknown `t` is logged and
   skipped. New snapshot fields may be added without raising `v`.

## Tests (in `cosmic-pass`)

Driven with the fake backend through the real `serve` loop:

- every intent reaches its `Msg`;
- a snapshot per state of spec FR-019;
- guarantees 1 to 3, including a refresh that reshapes the revealed item;
- a garbage line is skipped; closing stdin exits 0;
- `--clipboard-clear-secs` outside 10..600 exits 2.
