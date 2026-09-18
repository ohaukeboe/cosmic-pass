# Contract: `cosmic-pass` command line and D-Bus activation

## Invocations

| Command | Behavior | Exit |
|---------|----------|------|
| `cosmic-pass` | If no instance runs: become the resident instance and show the window. If one runs: send D-Bus `Activate` (toggle window) and exit. | 0 |
| `cosmic-pass --background` | Become the resident instance without showing the window (used by the systemd unit). If one already runs: exit without action. | 0 |
| `cosmic-pass show` | Resident instance shows the window (no toggle). Sent as `ActivateAction`. | 0 |
| `cosmic-pass hide` | Resident instance hides the window. | 0 |
| `cosmic-pass refresh` | Resident instance refreshes data in the background. | 0 |
| `cosmic-pass clipboard-serve --timeout <SECS> [--secret]` | Internal. Reads a value from stdin until EOF, offers it on the clipboard (see below), prints `ready` on stdout once it owns the selection, and exits when ownership is lost, when killed, or `SECS + 5` seconds after start. Not for direct use. | 0 when ownership lost; 2 on protocol unavailable; 3 on empty stdin |
| `cosmic-pass --version` / `--help` | Print and exit. | 0 |

Errors go to stderr; they MUST NOT contain secret values.

## D-Bus

Provided by libcosmic `single-instance`:

- Bus name / app ID: `io.github.ohaukeboe.CosmicPass`
- Object path: `/io/github/ohaukeboe/CosmicPass`
- Interface: `org.freedesktop.Application`
  - `Activate(platform_data)` → toggle window
  - `ActivateAction("<json RemoteAction>", [], platform_data)` → `"show"` | `"hide"` |
    `"refresh"` | `"background"` (no-op; sent by `--background` when an instance already runs)

`COSMIC_SINGLE_INSTANCE=0` disables single-instance (debug only).

## Environment overrides

| Variable | Purpose | Default |
|----------|---------|---------|
| `COSMIC_PASS_CLI` | Path to the `pass-cli` binary (tests use a fake). | `pass-cli` on `PATH` |
| `COSMIC_PASS_CACHE_DIR` | Cache directory (tests). | `$XDG_CACHE_HOME/cosmic-pass` |
| `COSMIC_PASS_NO_KEYRING` | `1` = treat keyring as unavailable (tests, privacy). | unset |
| `COSMIC_PASS_CLIPBOARD_HELPER` | Program run instead of `cosmic-pass clipboard-serve` (tests). | unset |

## `clipboard-serve` offer

MIME types offered, all from the same bytes except the hint:

| MIME | Value |
|------|-------|
| `text/plain;charset=utf-8` | value |
| `text/plain` | value |
| `UTF8_STRING` | value |
| `x-kde-passwordManagerHint` | `secret` (only when invoked with `--secret`) |

Protocol: `ext-data-control-v1`, falling back to `zwlr-data-control-v1`.

## Installed files

| Path | Purpose |
|------|---------|
| `$prefix/bin/cosmic-pass` | Binary |
| `$prefix/share/applications/io.github.ohaukeboe.CosmicPass.desktop` | Launcher entry (`Exec=cosmic-pass`) |
| `$prefix/lib/systemd/user/cosmic-pass.service` | Resident autostart (`ExecStart=cosmic-pass --background`) |
| `$prefix/share/metainfo/io.github.ohaukeboe.CosmicPass.metainfo.xml` | AppStream metadata |
| `$prefix/share/icons/hicolor/scalable/apps/io.github.ohaukeboe.CosmicPass.svg` | Icon |
