# Research: Proton Pass Quick Access

**Feature**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md) | **Date**: 2026-09-17

Research covered three areas: the `pass-cli` interface, COSMIC/libcosmic app patterns, and
Rust crate choices. Sources were read on 2026-09-17. Items marked *(inferred)* were not
confirmed from docs or source and each has a validation step in [quickstart.md](./quickstart.md).

---

## R1. UI toolkit

- **Decision**: `libcosmic` (git dependency, pinned `rev`), which wraps the pop-os fork of iced
  0.14. Features: `winit`, `wayland`, `tokio`, `single-instance`, `dbus-config`, `autosize`,
  `multi-window`, `wgpu`.
- **Renderer**: `wgpu` (GPU). Measured 2026-09-18 with 572 real items: the default tiny-skia
  CPU renderer redrew the 50-row list at ~46 ms per frame in a release build (far worse in a
  debug build, where typing felt like seconds per keystroke). Rendering, not search, is the
  cost: search is ~0.6 ms and the reducer update ~1 ms at that size.
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
    top (compositor centers it horizontally), fixed top offset 160 logical px (see
    *Top offset* below), width 640 px, `exclusive_zone: -1`.
  - Hide by destroying the layer surface. Hide on `LayerEvent::Unfocused`, on Escape, and
    after a copy. Use a 100 ms debounce so the toggle does not re-open a just-hidden window.
  - Running `cosmic-pass` again while an instance runs sends a D-Bus `Activate` to the resident
    instance, which toggles the window, then the new process exits.
- **Rationale**: This is exactly how COSMIC's own launcher meets the "instant open" goal
  (SC-001). The window never needs a cold process start.
- **Top offset (amended 2026-09-18)**: the original plan said "~20% of output height". The
  implementation uses a **fixed 160 logical px** (`surface::TOP_MARGIN`, rendered as a spacer
  above the framed popup inside the autosized surface), and that is the decision now.
  - Reasons: the app never learns which output the popup lands on. `output: None` lets the
    compositor place the surface on the focused output, while `OutputEvent::Created` /
    `InfoUpdate` only describe *all* outputs; with more than one output, picking a height
    would be a guess that changes where the popup sits depending on which monitor the user
    last touched. A proportional offset is also worse at the extremes: 20% of a 1440 px
    display pushes the popup 288 px down, while on a 768 px laptop panel 20% plus the list
    height starts to crowd the bottom of the screen.
  - A fixed offset keeps the popup in the upper third on 1080p and 1440p, is stable while the
    user types (the offset does not depend on the result count, and the surface is anchored
    top so only its bottom edge grows), and needs no output subscription or state.
  - Revisit if users on very tall or very short outputs report the popup feeling misplaced;
    the machinery would then be a `wayland::Event::Output` subscription plus a per-output
    logical height, and the margin would move from the view spacer to the layer surface's
    `IcedMargin`.
- **Alternatives considered**: xdg toplevel window (cannot guarantee centered, on-top,
  exclusive keyboard); cold-starting the process per shortcut press (too slow for 100 ms);
  deriving the top offset from the active output's logical height (see *Top offset*).

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
  - Toolchain: Rust stable, edition 2024, from the `flake.nix` dev shell (the host `rustup` has
    no default toolchain). The dev shell also provides `pkg-config`, `wayland`, `libxkbcommon`,
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

## Live validation (2026-09-18)

The quickstart scenarios were driven against the real vault on the target machine. Verified:
the core copy flow and clipboard clear (V2), field and TOTP copies including the action list
(V3), clipboard hint and timeout behaviour (V4), the detail pane with reveal and a live TOTP
countdown (V6), the status panels for signed-out, tool-missing and unreachable-network (V5
subset, simulated), the encrypted cache and its keyring key (V7), SC-001 latency, and
responsiveness at 10,000 items with no dropped keystrokes (V9).

Defects found live and fixed the same day: the keyboard selection was never drawn (rows used a
button class that paints no selected state); non-secret copies were wiped from the clipboard
~95 s later by the helper watchdog; the `pass-cli` missing panel printed its URL twice; the
session debug log carried the account id. A first attempt at the clipboard fix removed
`--timeout` from the helper argv, which the parser requires — that broke copying entirely and
was caught by re-verification, then fixed by arming the watchdog only for `--secret` and adding
a test that parses the spawned argv with the real parser.

## Open validation items (not blockers)

| ID | Item | Validated in |
|----|------|--------------|
| V1 | Real `pass-cli` JSON shapes and error texts | Done 2026-09-17 (locked-session text still unobserved) |
| V2 | `iced_test` works with libcosmic elements | **No** (2026-09-17): at libcosmic `87ab817` the `iced_test` crate in the pop-os iced fork does not compile (`renderer::Style` gained `icon_color`/`scale_factor`, `runtime::Action` gained `Dnd`/`PlatformSpecific`). UI behavior stays covered by reducer tests plus quickstart V2–V6. Re-check when libcosmic is bumped. |
| V3 | Selection clears when helper is killed; hint mime offered | **Yes** (2026-09-17): killing the helper leaves "Nothing is copied"; the helper exits when another client copies; `x-kde-passwordManagerHint=secret` is offered. |
| V4 | `pass-cli login` needs no TTY | **Partly** (2026-09-20): run against a real `pass-cli logout`. The signed-out panel appeared and the login URL reached it, so `pass-cli login` does stream its URL without a TTY. Pressing the link opened no browser: `xdg-open` is not on the systemd unit's PATH, so the spawn fails with `os error 2` and the popup hides with no visible error (cosmic-pass-wqx.51). The journal also keeps the full sign-in URL, payload included (cosmic-pass-wqx.52). |
| V6 | Focus loss hides the popup (FR-003) | **Yes** (2026-09-20): with the popup open, clicking into another window hid it without Escape. |
| V7 | Locked-keyring path (FR-024a) | **Yes** (2026-09-20): the gnome-keyring `login` collection was locked over the Secret Service (`org.freedesktop.Secret.Service.Lock`) and the app restarted. It stayed usable and `~/.cache/cosmic-pass/cache.bin` was not rewritten, which is the `KeyState::Unavailable` path FR-024a asks for. |
| V5 | Typing reaches the search field | Fixed 2026-09-18: the field needs `always_active()` plus a focus task on `LayerEvent::Focused`; an early focus task alone is lost while the layer surface is being created. |

### Open latency (SC-001)

The app measures each open itself: the clock starts when the show request reaches `dispatch`
(`Msg::Show` / `Msg::Toggle`, including the D-Bus `Activate` path used by the global shortcut
and by a second `cosmic-pass` invocation) and stops at `LayerEvent::Focused` for our layer
surface, the point where the popup accepts typing. The result is one debug line per open:

```
open latency: 42.3 ms from show request to focus
```

To capture it:

```bash
systemctl --user stop cosmic-pass    # if the service is running
RUST_LOG=cosmic_pass=debug ./target/release/cosmic-pass --background 2>&1 | tee /tmp/open-latency.log
# press the shortcut ~20 times, then:
grep -o 'open latency: [0-9.]*' /tmp/open-latency.log | sort -g -k3 | tail -3
```

Use a release build; a debug build is not representative. The first open after start also pays
the cold-start cost and is reported separately from the steady-state figures. No item data is
logged — the line carries a duration only.

**Scope:** the clock starts inside the resident instance, so the logged figure excludes the
launcher process that the shortcut spawns and its D-Bus hop to the running instance. SC-001
budgets the whole press-to-typing path, so pair the logged p95 with a `time cosmic-pass` run
against a live instance before judging the criterion met.

**Measured 2026-09-18** on the target machine (release build, 572-item vault), driven
synthetically while the user stayed off the keyboard:

| Segment | median | p95 | max |
|---------|--------|-----|-----|
| In-app (show request → `LayerEvent::Focused`), warm, n=55 | 22.3 ms | 40.9 ms | 46.6 ms |
| Shortcut process spawn + D-Bus `Activate` hop, n=15 | 8.7 ms | 10.6 ms | 10.8 ms |
| End to end (paired per run, n=15) | 30.6 ms | 52.2 ms | 55.3 ms |

SC-001 (< 100 ms in 95% of openings) is met with about 2× headroom. Known outlier: the first
open after process start measured 87–123 ms across runs; a resident instance pays this once per
process lifetime, not per shortcut press, so it does not move the 95th percentile of real usage.


