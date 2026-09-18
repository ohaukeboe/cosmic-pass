# Contract: Consumed `pass-cli` interface (2.3.x)

This is the interface the app depends on. It was confirmed against `pass-cli` 2.3.3 on
2026-09-17 with a signed-in account (quickstart V1). Redacted real output lives in
`tests/fixtures/pass-cli/captured/`; hand-written variants live in
`tests/fixtures/pass-cli/synthetic/`.

## Invocation rules

- Binary: `$COSMIC_PASS_CLI` or `pass-cli` on `PATH`.
- Env added: `PASS_LOG_LEVEL=off`, `PROTON_PASS_NO_UPDATE_CHECK=1`.
- `stdin`: null. `stdout`, `stderr`: piped.
- Own process group; killed on drop, timeout, or cancel.
- Timeouts: `info` 5 s, `vault list` / `item list` 20 s, `item view` / `item totp` 10 s,
  `login` 5 min.
- Concurrency: at most 4 processes at once.
- Secret values are never passed in argv.
- **IDs MUST be passed as `--flag=VALUE`.** Share IDs can start with `-`; the form
  `--share-id VALUE` then fails with `unexpected argument`.

Measured latency: `info` and `vault list` ~0.6 s; `item list --show-secrets` for two vaults
(616 items) ~3.6 s total.

## Commands

### `pass-cli info --output json`

Exit 0 when signed in:

```json
{"release_track": "…", "id": "…", "username": "…", "email": "…", "session_has_lock": false}
```

`id` is the `AccountId`.

### `pass-cli vault list --output json`

```json
{"vaults": [{"name": "…", "vault_id": "…", "share_id": "…"}]}
```

### `pass-cli item list --share-id=<SHARE_ID> --output json --show-secrets`

The plain form (without `--show-secrets`) returns only `id`, `share_id`, `vault_id`,
`state`, `flags`, `create_time`, `modify_time`, `title`, `item_type`. It has no username,
URLs, or TOTP indicator, so the app uses `--show-secrets` and strips every secret inside the
parser (the raw buffer is zeroized).

```json
{"items": [{
  "id": "…", "share_id": "…", "vault_id": "…",
  "state": "Active | Trashed",
  "flags": ["ItemHasFiles", "ItemHasHadFiles"],
  "create_time": "YYYY-MM-DDTHH:MM:SS", "modify_time": "YYYY-MM-DDTHH:MM:SS",
  "content": {
    "title": "…", "note": "<secret>", "item_uuid": "…",
    "content": { "<Kind>": <kind object or null> },
    "extra_fields": [{"name": "…", "content": {"Text" | "Hidden" | "Totp": "…"}}]
  }
}]}
```

`<Kind>` is the single key of `content.content`:

| Kind | Non-secret fields kept | Secret fields (never kept) |
|------|------------------------|----------------------------|
| `Login` | `username`, `email`, `urls[]` | `password`, `totp_uri` (only its non-emptiness is kept), `passkeys` |
| `Note` (`null`) | — | top-level `content.note` |
| `CreditCard` | `cardholder_name`, `card_type`, `expiration_date` | `number`, `verification_number`, `pin` |
| `Identity` | `full_name`, `email` | all other fields |
| `Wifi` | `ssid`, `security` | `password` |
| `SshKey` | `public_key` | `private_key` |
| `Custom` | — | — |
| `Alias` *(not seen in the test account)* | — | — |
| other | — | everything |

Custom fields: `content.extra_fields[]` and, for `SshKey`/`Wifi`/`Custom`,
`content.content.<Kind>.sections[].section_fields[]`. Each has `name` and
`content: {Text|Hidden|Totp: value}`. Only the name and the variant are kept; values are
dropped. `Text` values are also dropped (they may hold personal data).

`content.note` is secret for every kind.

The plain form's `item_type` values are `login`, `note`, `credit_card`, `identity`, `alias`,
`ssh_key`, `wifi`, `custom`.

### `pass-cli item view --share-id=<S> --item-id=<I> --field=<FIELD>`

Prints the raw field value followed by a newline. The output is **not JSON**, even with
`--output json`. The app strips one trailing `\n` and wraps the value in `SecretString`.

Field names are the JSON keys above: `password`, `username`, `email`, `urls`, `totp_uri`,
`number`, `verification_number`, `pin`, `cardholder_name`, `expiration_date`, `note`,
`ssid`, `private_key`, `public_key`, and custom field names. An empty field fails with
`Error: Field does not exist: <FIELD>`.

Non-secret fields (username, email, first URL) are copied from the cached summary without
calling `pass-cli`.

### `pass-cli item totp --share-id=<S> --item-id=<I> --output json`

```json
{"totp": "123456", "totp_uri": "123456", "<custom TOTP field name>": "654321"}
```

The login's own code is under `totp_uri` (with `totp` as a duplicate). Custom TOTP fields
appear under their names. No period is returned; the app assumes 30 s.

### `pass-cli login`

Web login. Tries to open the browser; prints a URL when it cannot. Exits 0 when the browser
flow completes. The app shows any printed URL in the window as a clickable link.
*Not yet verified without a TTY (quickstart V5).*

## Error format

All failures exit 1. Stderr looks like:

```text
Error: Error retrieving item

Caused by:
    0: Error finding item by name
    1: Error finding vault by name
    2: Could not find vault <ID>
```

With `PASS_LOG_LEVEL` unset, a coloured tracing line may come first.

## Error mapping

The app matches against the whole stderr text (case-insensitive), in this order:

| Match | Error |
|-------|-------|
| spawn fails with `NotFound` | `CliMissing` |
| process exceeded app timeout | `Timeout` |
| `requires an authenticated client`, `there is no session` | `SignedOut` |
| `field does not exist` | `FieldMissing` |
| `locked` | `Locked` *(text not yet observed)* |
| `could not find`, `error finding item`, `idformat` | `NotFound` |
| `connection`, `timed out`, `dns`, `network` | `Network` |
| anything else | `Cli { message }`: the first `Error:` line, truncated to 200 chars |

Unparseable stdout with exit 0 → `Protocol { command }`; the raw output is not logged.
