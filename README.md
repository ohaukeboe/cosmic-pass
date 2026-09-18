# COSMIC Pass

A keyboard-driven quick-access popup for [Proton Pass](https://proton.me/pass) on the
[COSMIC](https://system76.com/cosmic) desktop, similar to 1Password Quick Access.

Press a shortcut, type a few letters, press Enter: the password is on your clipboard and the
popup is gone.

> Unofficial. Not affiliated with or endorsed by Proton AG. All communication with Proton
> Pass goes through the official
> [`pass-cli`](https://protonpass.github.io/pass-cli/) command-line tool.

## Requirements

- COSMIC desktop (Wayland).
- `pass-cli` 2.3 or newer on `PATH`, signed in (`pass-cli login`).
- A Secret Service provider (for example gnome-keyring) for the encrypted item cache.
- A non-sandboxed install: the clipboard helper needs the Wayland data-control protocol.

## Install

```bash
nix-shell          # or: direnv allow
just install-user
systemctl --user daemon-reload
systemctl --user enable --now cosmic-pass.service
```

Then add the shortcut: **COSMIC Settings → Keyboard → Keyboard Shortcuts → Custom → Add**,
command `cosmic-pass`, keys `Super+Shift+P` (or any keys you like). Running `cosmic-pass`
toggles the popup; if the background service is not running, the first run starts it.

## Keys

| Key | Action |
|-----|--------|
| type | Search titles, usernames, websites, and vault names |
| `Enter` | Copy the password (card number, note, ...) and close |
| `↑` / `↓`, `Ctrl+P` / `Ctrl+N` | Move the selection |
| `Esc` | Cancel, go back, or close |
| `F5` | Refresh items |

The full keyboard contract is in
[`specs/001-quick-access-launcher/contracts/keyboard.md`](specs/001-quick-access-launcher/contracts/keyboard.md).

## Security notes

- Secret values are fetched from `pass-cli` only when you copy or reveal them, and are never
  written to disk or logs.
- Copied secrets are removed from the clipboard after 90 seconds, but only if they are still
  the current clipboard content.
- Copies carry the `x-kde-passwordManagerHint=secret` hint so clipboard managers that honor
  it skip them. COSMIC's clipboard manager does not honor this hint yet, so secrets may appear
  in its history.
- Search metadata (titles, usernames, websites, vault names) is cached on disk encrypted with a
  key held in your keyring. Without an unlocked keyring nothing is written.

## Development

See the Build & Test section in [`CLAUDE.md`](CLAUDE.md). Design documents live in
[`specs/001-quick-access-launcher/`](specs/001-quick-access-launcher/).

## License

GPL-3.0-only.
