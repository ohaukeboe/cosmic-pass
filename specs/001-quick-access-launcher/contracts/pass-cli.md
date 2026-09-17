# Contract: Consumed `pass-cli` interface (2.3.x)

This is the interface the app depends on. It is based on `pass-cli --help`, the official docs
(https://protonpass.github.io/pass-cli/), and local tests on 2.3.3. JSON shapes marked
*unverified* MUST be confirmed against real output (quickstart V1) and recorded as fixtures.

## Invocation rules

- Binary: `$COSMIC_PASS_CLI` or `pass-cli` on `PATH`.
- Env added: `PASS_LOG_LEVEL=off`, `PROTON_PASS_NO_UPDATE_CHECK=1`.
- `stdin`: null (except `login`). `stdout`, `stderr`: piped.
- Always pass `--output json`.
- Own process group; killed on drop, timeout, or cancel.
- Timeouts: `info` 5 s, `vault list` / `item list` 20 s, `item view` / `item totp` 10 s,
  `login` 5 min.
- Concurrency: at most 4 processes at once.
- Secret values are never passed in argv.

## Commands

### `pass-cli info --output json`

Success: exit 0; JSON object with the account identity. *Unverified keys*; the parser reads
the first present of `user_id`, `userId`, `id`, then `email`/`username` as a fallback.

### `pass-cli vault list --output json`

Success: exit 0; JSON array (or object with `vaults` array) of vaults. *Unverified keys*;
parser reads `share_id`|`shareId` and `name`|`title`.

### `pass-cli item list --share-id <SHARE_ID> --filter-state active --output json [--show-secrets]`

Success: exit 0; JSON array (or object with `items` array). *Unverified keys*. Parser reads:

| Summary field | Candidate JSON paths |
|---------------|---------------------|
| item id | `id`, `item_id`, `itemId` |
| share id | `share_id`, `shareId` (fallback: the requested share) |
| title | `title`, `content.title`, `metadata.name` |
| kind | `type`, `item_type`, `content.type` (lowercase: `login`, `note`, `credit-card`/`credit_card`, `identity`, `alias`, `ssh-key`, `wifi`, `custom`) |
| state | `state` (`active`/`trashed`) |
| username | `username`, `content.username`, `content.item_username`, `content.item_email`, `content.email` |
| urls | `urls`, `content.urls` |
| has TOTP | `has_totp`, or non-empty `content.totp_uri` (value discarded) |
| modified | `modify_time`, `modified_at`, `update_time` |

`--show-secrets` is used only if the plain form lacks username/urls/TOTP information. In that
case every secret-bearing key is dropped inside the parser and the raw buffer is zeroized.

### `pass-cli item view pass://<SHARE_ID>/<ITEM_ID>/<FIELD> --output json`

Success: exit 0; the field value. *Unverified shape*; parser accepts a JSON string, or an
object with `value`/`<FIELD>`. Result is wrapped in `SecretString` immediately.

Field names: `username`, `password`, `email`, `url`, `note`, `totp`, custom names,
`Section.field`. Credit card field names are *unverified* (candidates: `number`,
`card_number`, `cvv`, `verification_number`, `pin`, `holder`, `cardholder_name`,
`expiration_date`).

### `pass-cli item totp pass://<SHARE_ID>/<ITEM_ID> --output json [--field <FIELD>]`

Success: exit 0; JSON object mapping TOTP field name → code, e.g.
`{"totp": "152470"}` (documented). No period is returned; the app assumes 30 s.

### `pass-cli login`

Web login. Tries to open the browser; prints a URL when it cannot. Exits 0 when the browser
flow completes. The app shows any printed URL in the window as a clickable link.

## Error mapping

All failures exit 1. The app reads the last stderr line starting with `Error:`.

| Match (case-insensitive) | Error |
|--------------------------|-------|
| spawn fails with `NotFound` | `CliMissing` |
| `requires an authenticated client`, `no session` | `SignedOut` |
| `locked` | `Locked` *(unverified text)* |
| `connection`, `timed out`, `dns`, `network` | `Network` |
| process exceeded app timeout | `Timeout` |
| `not found` (item/vault) | `NotFound` |
| anything else | `Cli { message }` (message shown, truncated to 200 chars) |

Unparseable stdout with exit 0 → `Protocol { command }`; the raw output is not logged.
