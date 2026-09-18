# Research: Proton Pass Quick Access

**Feature**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md) | **Date**: 2026-09-17

Research covered three areas: the `pass-cli` interface, COSMIC/libcosmic app patterns, and
Rust crate choices. Sources were read on 2026-09-17. Items marked *(inferred)* were not
confirmed from docs or source and each has a validation step in [quickstart.md](./quickstart.md).

---

## R1. UI toolkit

- **Decision**: `libcosmic` (git dependency, pinned `rev`), which wraps the pop-os fork of iced
  0.14. Features: `winit`, `wayland`, `tokio`, `single-instance`, `dbus-config`, `autosize`,
  `multi-window`.
- **Rationale**: The request asks for iced on COSMIC. libcosmic is COSMIC's official iced-based
  toolkit (used by `cosmic-app-template` and `cosmic-launcher`). It gives native theming,
  layer-shell surfaces, single-instance D-Bus activation, and `cosmic-config`. iced is reached
  through `cosmic::iced`.
- **Alternatives considered**: Plain upstream iced: no layer-shell, no COSMIC theme, no
  single-instance support; we would rebuild all three.
- **Note**: libcosmic is not on crates.io. Pin the git `rev` in `Cargo.toml` and keep
  `Cargo.lock` committed. Any `iced_test` dependency MUST use the same pop-os/iced rev.

## R2. Window type, toggling, and process model

- **Decision**: Copy `cosmic-launcher`'s model.
  - Start with `cosmic::app::run_single_instance`, `no_main_window(true)`,
    `exit_on_close(false)`. The process stays resident.
  - Show the UI as a wlr layer-shell surface: `KeyboardInteractivity::Exclusive`, anchored
    top (compositor centers it horizontally), top margin ~20% of output height, width 640 px,
    `exclusive_zone: -1`.
  - Hide by destroying the layer surface. Hide on `LayerEvent::Unfocused`, on Escape, and
    after a copy. Use a 100 ms debounce so the toggle does not re-open a just-hidden window.
  - Running `cosmic-pass` again while an instance runs sends a D-Bus `Activate` to the resident
    instance, which toggles the window, then the new process exits.
- **Rationale**: This is exactly how COSMIC's own launcher meets the "instant open" goal
  (SC-001). The window never needs a cold process start.
- **Alternatives considered**: xdg toplevel window (cannot guarantee centered, on-top,
  exclusive keyboard); cold-starting the process per shortcut press (too slow for 100 ms).

## R3. Global shortcut and autostart

- **Decision**:
  - Shortcut: the user adds a custom shortcut in COSMIC Settings that runs `cosmic-pass`
    (suggested `Super+Shift+P`). The README documents it. The app does not write COSMIC
    shortcut config in v1.
  - Autostart: ship a systemd user unit `cosmic-pass.service`
    (`PartOf=graphical-session.target`, `WantedBy=graphical-session.target`,
    `Restart=on-failure`). If the service is not running, the shortcut still works: the first
    invocation becomes the resident instance and shows the window.
- **Rationale**: COSMIC stores default shortcuts in one file owned by cosmic-comp; apps have no
  drop-in location. Writing the user's `custom` file silently would violate user control.
  `cosmic-session.target` binds to `graphical-session.target`, so the unit starts with COSMIC.
- **Alternatives considered**: XDG autostart `.desktop` (works, but no restart-on-failure or
  journald logs); an opt-in "add shortcut" action via `cosmic-settings-config` (deferred).

## R4. Talking to Proton Pass (`pass-cli` 2.3.x)

- **Decision**: Wrap `pass-cli` behind a `PassBackend` trait, implemented by `PassCli`, which
  spawns the binary through a `CommandRunner` trait. Commands used:

  | Purpose | Command |
  |---------|---------|
  | Session / account | `pass-cli info --output json` |
  | Vaults | `pass-cli vault list --output json` |
  | Items per vault | `pass-cli item list --share-id=<ID> --output json --show-secrets` |
  | One field | `pass-cli item view --share-id=<S> --item-id=<I> --field=<F>` |
  | One-time code | `pass-cli item totp --share-id=<S> --item-id=<I> --output json` |
  | Sign in | `pass-cli login` (web flow) |

  Full contract: [contracts/pass-cli.md](./contracts/pass-cli.md).
- **Rationale**: The request requires `pass-cli`. A trait boundary lets unit tests use fakes
  and integration tests use a fake `pass-cli` script (constitution Principles II, III).
- **Facts found**:
  - `item list` takes one vault per call; no all-vaults listing exists. List vaults, then
    list items per vault in parallel.
  - Field names are the JSON keys of the item content (`password`, `number`, `totp_uri`,
    `private_key`, custom field names). Empty fields report `Field does not exist`.
  - `item totp --output json` returns a map `{ "<field name>": "<code>" }` with no period.
    Countdown is computed locally assuming a 30 s period.
  - All errors exit with code 1. Errors are told apart by stderr text.
    Signed out: `Error: This operation requires an authenticated client`.
  - Logs go to stderr; `PASS_LOG_LEVEL=off` silences them.
  - `pass-cli login` (web) tries to open a browser and prints the URL on failure; no TTY
    required *(inferred)*.
  - Share IDs can change across sessions. Cache keys MUST be re-validated after each refresh.
- **JSON schema (confirmed 2026-09-17, quickstart V1)**: shapes are recorded in
  [contracts/pass-cli.md](./contracts/pass-cli.md) and `tests/fixtures/pass-cli/captured/`.
  Key findings:
  - Plain `item list` lacks username, URLs, and TOTP data, so the app uses
    `--show-secrets` and strips every secret inside the parser. The raw output buffer is
    zeroized and dropped immediately (FR-014, FR-025).
  - Item kind is the single key of `content.content` (`Login`, `Note`, `CreditCard`, ...).
  - `item view --field` prints the raw value, not JSON. Non-secret fields (username, email,
    URL) are copied from the summary without a `pass-cli` call.
  - Share IDs can start with `-`; every ID is passed as `--flag=VALUE`.
  - Errors are `Error: ...` plus a `Caused by:` chain; the classifier matches the whole text.
- **Invocation environment**: `PASS_LOG_LEVEL=off`, `PROTON_PASS_NO_UPDATE_CHECK=1`,
  `stdin` null, `kill_on_drop(true)`, own process group. Timeouts: 20 s for listing, 10 s
  for field reads. At most 4 concurrent `pass-cli` processes (semaphore).
- **Error classification**: see the ordered table in
  [contracts/pass-cli.md](./contracts/pass-cli.md#error-mapping).
- **Alternatives considered**: Talking to Proton's API directly (rejected by request and far
  more security-sensitive); `pass-cli run`/`inject` (built for env/file injection, not
  interactive lookup).

## R5. Clipboard with sensitive hint and safe clearing

- **Decision**: A clipboard helper subprocess. The app re-executes itself as
  `cosmic-pass clipboard-serve --timeout <secs>` and writes the secret to its stdin. The
  helper uses `wl-clipboard-rs` `copy_multi` on the `ext-data-control` / `wlr-data-control`
  protocol, offering:
  - `text/plain;charset=utf-8` and `text/plain` = the secret
  - `x-kde-passwordManagerHint` = `secret`

  The helper serves in the foreground. It exits by itself when another client takes the
  selection (ownership lost). The parent kills it when the timeout expires. Destroying the
  data source clears the selection only if the helper still owns it.
- **Rationale**:
  - The window closes right after a copy, so toolkit clipboard (tied to window focus) would
    lose the data. Data-control works without focus.
  - "Clear only if still owned" (FR-015) falls out naturally: the helper is only alive while
    it owns the selection.
  - `wl-clipboard-rs` has no cancel handle for a foreground copy; a subprocess gives a clean
    kill switch and keeps secret bytes out of the resident process after hand-off.
- **Facts found**: cosmic-comp exposes both data-control protocols to non-sandboxed clients.
  COSMIC's clipboard manager does not honor `x-kde-passwordManagerHint` today; we still set
  it (KDE and others honor it) and document the gap.
- **Risks** *(inferred)*: compositor clears the selection when the source is destroyed;
  Flatpak builds lose data-control access. Validation: quickstart V4.
- **Alternatives considered**: `arboard` with `exclude_from_history().wait()` (no cancel
  handle, would need a thread we cannot stop); libcosmic `clipboard::write_data` (needs
  focus, ends with window); a hand-written `wayland-client` data-control client (most
  control, more code; revisit if the helper proves unreliable).

## R6. Search

- **Decision**: `nucleo-matcher` 0.3.x, run synchronously in `update()` on every keystroke over
  a prebuilt haystack (title, username, URLs host, vault name). Score = best field score with a
  title weight bonus. Show top 50 results. Empty query: recent items, then alphabetical.
- **Rationale**: fzf-style scoring, used by Helix, matching 5,000 short strings in well under
  1 ms *(inferred)*, so a background task is unnecessary (Principle IV). A benchmark test
  guards SC-002.
- **Alternatives considered**: `nucleo` (threaded, not needed at this scale); `frizbee`
  (typo tolerance, newer API; revisit if users ask for typo tolerance); `fuzzy-matcher`
  (unmaintained since 2020).

## R7. Encrypted metadata cache

- **Decision**:
  - Key: 32 random bytes stored in the Secret Service via `oo7` (async, tokio), attributes
    `application=io.github.ohaukeboe.CosmicPass`, `purpose=cache-key`.
  - Cipher: `chacha20poly1305` (XChaCha20-Poly1305, random 24-byte nonce per write).
  - Encoding: `postcard` + `serde`.
  - File: `$XDG_CACHE_HOME/cosmic-pass/cache.bin`, mode `0600`, written atomically
    (`tempfile` in same dir → `sync_all` → `persist` → fsync dir).
  - In-memory secrets: `secrecy` + `zeroize`.
  - Format: [contracts/cache-format.md](./contracts/cache-format.md).
- **Rationale**: Meets FR-024 (instant results after login/reboot) without leaking which
  sites the user has accounts at. `oo7` is pure Rust and async, matching the tokio runtime.
- **Behavior**:
  - Keyring unavailable/locked → no disk writes, memory only (FR-024a).
  - Decrypt fails or account mismatch → delete file, refetch.
  - Sign-out or account switch → delete file and keyring item (FR-024b).
- **Alternatives considered**: `keyring` 4.x (sync API; store-crate restructure is new);
  `bincode` (unmaintained, RUSTSEC-2025-0141); `aes-gcm` (fine, but XChaCha avoids nonce
  management pitfalls).

## R8. Refresh policy and responsiveness

- **Decision**:
  - On startup: load cache (if any) → show immediately → refresh in background.
  - On window open: refresh in background if last successful refresh is older than 5 min.
  - Manual refresh: `F5`.
  - Failed refresh: keep old data, show stale indicator, retry on next open.
  - Secret fetch: busy indicator on the row; Escape cancels and drops the result.
  - All `pass-cli` work runs in `cosmic::task::future` on tokio; `update()` and `view()` never
    block.
- **Rationale**: Meets FR-022, FR-023, SC-001, SC-004.

## R9. Preferences

- **Decision**: `cosmic-config` (libcosmic `dbus-config`), config ID
  `io.github.ohaukeboe.CosmicPass`, version 1. Keys: `clipboard_clear_secs` (default 90,
  range 10–600), `shortcuts` (action → key chord), `refresh_stale_secs` (default 300).
  Schema: [contracts/config.md](./contracts/config.md).
- **Rationale**: Native COSMIC config with live watch; no extra dependency.

## R10. Tooling and testing

- **Decision**:
  - Toolchain: Rust stable, edition 2024, from `shell.nix` (the host `rustup` has no default
    toolchain). `shell.nix` also provides `pkg-config`, `wayland`, `libxkbcommon`,
    `vulkan-loader`, `mesa`, `fontconfig`, `freetype`, `expat`, `just`, `cargo-nextest`,
    `cargo-llvm-cov`.
  - Gates via `just`: `just fmt` (`cargo fmt --check`), `just lint`
    (`cargo clippy --all-targets -- -D warnings`), `just test` (`cargo nextest run`),
    `just cov` (`cargo llvm-cov nextest`, 80% line floor on changed code), `just check`
    (all of these).
  - Unit tests: pure `core` reducer and parsers; fakes for `CommandRunner`, `PassBackend`,
    `Clipboard`, `KeyStore`.
  - Integration tests: `tests/` against `tests/fixtures/fake-pass-cli` (shell script driven
    by env vars and fixture JSON), pointed to via `COSMIC_PASS_CLI` env override.
  - Snapshots: `insta` for parsed `pass-cli` fixtures.
  - UI: `iced_test` does not compile against the pinned libcosmic (see V2), so UI behavior is
    covered by reducer and acceptance tests plus manual quickstart checks.
  - Benchmark: search over 5,000 generated items, asserted under 50 ms (SC-002) in a test
    marked `#[ignore]` and run in `just bench`. Measured 2026-09-17: p95 0.6 ms per search,
    0.7 ms per reducer update.
- **Rationale**: Satisfies constitution Quality Standards (single commands, reproducible
  environment) and Principles II–III.

## Open validation items (not blockers)

| ID | Item | Validated in |
|----|------|--------------|
| V1 | Real `pass-cli` JSON shapes and error texts | Done 2026-09-17 (locked-session text still unobserved) |
| V2 | `iced_test` works with libcosmic elements | **No** (2026-09-17): at libcosmic `87ab817` the `iced_test` crate in the pop-os iced fork does not compile (`renderer::Style` gained `icon_color`/`scale_factor`, `runtime::Action` gained `Dnd`/`PlatformSpecific`). UI behavior stays covered by reducer tests plus quickstart V2–V6. Re-check when libcosmic is bumped. |
| V3 | Selection clears when helper is killed; hint mime offered | **Yes** (2026-09-17): killing the helper leaves "Nothing is copied"; the helper exits when another client copies; `x-kde-passwordManagerHint=secret` is offered. |
| V4 | `pass-cli login` needs no TTY | quickstart V5 |
