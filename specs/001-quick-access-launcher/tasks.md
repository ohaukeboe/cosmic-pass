---

description: "Task list for Proton Pass Quick Access"
---

# Tasks: Proton Pass Quick Access

**Input**: Design documents from `/specs/001-quick-access-launcher/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

**Tests**: Required. The constitution (Principle II, Test-First, NON-NEGOTIABLE) mandates a
failing test before each behavior. Every test task MUST be written and seen failing before
its paired implementation task.

**Organization**: Tasks are grouped by user story. Story phases follow spec priority:
US1 (P1) → US2 (P2) → US4 (P2) → US3 (P3). Encrypted cache and preferences are
cross-cutting and come after the stories.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: User story from spec.md (US1–US4)
- Paths are relative to the repository root (single Rust crate, see plan.md)

## Conventions for every task

- Run `just check` (fmt, clippy `-D warnings`, nextest, coverage) before marking a task done.
- Never log, `Debug`-print, serialize, or put in argv any secret value. Use
  `secrecy::SecretString` for secrets.
- Unit tests live in `#[cfg(test)] mod tests` inside the named source file unless a `tests/`
  path is given.
- Test doubles: `FakeBackend` (implements `PassBackend`), `FakeClipboard` (implements
  `Clipboard`), `FakeKeyStore` (implements `KeyStore`), `FakeRunner` (implements
  `CommandRunner`). They live in `src/testing.rs` behind `#[cfg(any(test, feature = "testing"))]`.
- `pass-cli` behaviour is specified in
  [contracts/pass-cli.md](./contracts/pass-cli.md). Use it for command lines, timeouts, and
  error mapping.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Reproducible toolchain, crate skeleton, and quality gates.

- [x] T001 Replace `shell.nix` with a dev shell providing: `rustc`, `cargo`, `clippy`, `rustfmt`, `rust-analyzer`, `pkg-config`, `just`, `cargo-nextest`, `cargo-llvm-cov`, `jq`, `wl-clipboard`, and libraries `wayland`, `libxkbcommon`, `vulkan-loader`, `mesa`, `fontconfig`, `freetype`, `expat`, `libGL`; export `LD_LIBRARY_PATH` for `wayland`, `libxkbcommon`, `vulkan-loader`, `libGL` so `cargo run` works. Verify with `nix-shell --run 'cargo --version && just --version'`.
- [x] T002 Create `Cargo.toml`: package `cosmic-pass`, edition 2024, `rust-version` = libcosmic's MSRV, `[lib] path = "src/lib.rs"`, `[[bin]] name = "cosmic-pass"`, feature `testing = []`. Dependencies: `libcosmic` (git `https://github.com/pop-os/libcosmic`, pinned `rev` = current `master` from `git ls-remote`, `default-features = false`, features `winit`, `wayland`, `tokio`, `single-instance`, `dbus-config`, `autosize`, `multi-window`), `tokio` (`rt-multi-thread`, `process`, `time`, `sync`, `macros`, `io-util`), `tokio-util`, `serde` (`derive`), `serde_json`, `clap` (`derive`), `nucleo-matcher` 0.3, `wl-clipboard-rs` 0.9, `oo7` 0.6 (`tokio`, `native_crypto`), `chacha20poly1305` 0.11, `rand` (for nonces/keys), `postcard` (`use-std`), `tempfile`, `dirs`, `secrecy` 0.10, `zeroize`, `thiserror`, `tracing`, `tracing-subscriber`, `rustix` (`process`). Dev-dependencies: `insta` (`json`), `tempfile`, `tokio` (`test-util`). Add `[lints.rust] unsafe_code = "forbid"` and `[lints.clippy] all = "warn"`. Commit `Cargo.lock`.
- [x] T003 [P] Add `rustfmt.toml` (`edition = "2024"`, `max_width = 100`) and `clippy.toml` (`too-many-lines-threshold = 50`, `cognitive-complexity-threshold = 10`) at repo root.
- [x] T004 [P] Create `justfile` with recipes: `fmt` (`cargo fmt --all --check`), `lint` (`cargo clippy --all-targets --all-features -- -D warnings`), `test` (`cargo nextest run --all-features`), `cov` (`cargo llvm-cov nextest --all-features --fail-under-lines 80`), `check` (fmt lint test cov), `run` (`cargo run --`), `bench` (`cargo nextest run --release --run-ignored only search_bench`), `install-user`, `capture-fixtures`, `leak-scan` (the last three may call placeholder recipes until T049, T010, T079).
- [x] T005 Create the module skeleton from plan.md so `cargo build` succeeds: `src/main.rs`, `src/lib.rs` (declares `app`, `core`, `pass`, `cache`, `clipboard`, `model`, `config`, `testing`), `src/app/mod.rs`, `src/app/surface.rs`, `src/app/keys.rs`, `src/app/view/{mod,list,actions,detail,preferences,status}.rs`, `src/core/{mod,state,effects,search,actions,totp,usage}.rs`, `src/pass/{mod,runner,backend,parse,error}.rs`, `src/cache/{mod,keystore,crypto,store}.rs`, `src/clipboard/{mod,serve}.rs`, `src/model.rs`, `src/config.rs`, `src/testing.rs`. Each file holds only a module doc comment.
- [x] T006 [P] Add `target/` and `*.profraw` to `.gitignore`.
- [x] T007 [P] Fill the "Build & Test" section in both `CLAUDE.md` and `AGENTS.md` with: `nix-shell` (or `direnv allow`), `just check`, `just test`, `just run`, `just bench`. Also add a one-line architecture overview matching plan.md.

**Checkpoint**: `nix-shell --run 'just fmt && just lint && just test'` passes on the empty crate.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Domain types, the `pass-cli` boundary, search, the reducer skeleton, and a
resident window that can be toggled. Every story needs these.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### Test infrastructure

- [x] T008 [P] Create `tests/fixtures/fake-pass-cli` (POSIX `sh`, executable). Behaviour: append its argv (one line, space-joined) to `$FAKE_ARGV_LOG` if set; sleep `$FAKE_SLEEP` seconds if set; if `$FAKE_EXIT` is set and non-zero, print `$FAKE_STDERR` (default `Error: fake failure`) to stderr and exit with it; otherwise map the command to a fixture file in `$FAKE_FIXTURE_DIR` (`info` → `info.json`, `vault list` → `vault-list.json`, `item list --share-id X` → `item-list-X.json`, `item view pass://S/I/F` → `item-view-S-I-F.json`, `item totp pass://S/I` → `item-totp-S-I.json`) and print it; if `$FAKE_ITEMS=N` and the command is `item list`, print a generated JSON array of N login items instead. Unknown commands exit 1 with `Error: unknown fake command`.
- [x] T009 [P] Create `tests/fixtures/pass-cli/synthetic/` with hand-written JSON fixtures covering every shape variant in contracts/pass-cli.md: array vs `{ "vaults": [...] }` / `{ "items": [...] }` wrappers, alternative key names (`share_id`/`shareId`, `id`/`item_id`), nested `content.*` fields, all item kinds including `credit-card` and `credit_card`, one `trashed` item, one item of unknown kind `passkey-thing`, and secret fields (`password`, `totp_uri`, `number`, `cvv`, `pin`, note body, hidden custom field, SSH private key, Wi-Fi password). Every secret value MUST contain the marker `SECRET-FIXTURE-` so leak tests can grep for it.
- [x] T010 [P] Create `scripts/capture-fixtures.sh` (bash, called by `just capture-fixtures`): requires a signed-in `pass-cli`; runs every command listed in contracts/pass-cli.md with `--output json` (plain and `--show-secrets` for `item list`) plus `pass-cli info` while signed out if `CAPTURE_SIGNED_OUT=1`; pipes output through `jq` to replace every string value with a type-preserving placeholder (`"REDACTED-<key>"`, secrets become `"SECRET-FIXTURE-<key>"`) while keeping keys and structure; writes to `tests/fixtures/pass-cli/captured/`. Also records stderr of failing commands to `tests/fixtures/pass-cli/captured/errors.txt` with personal data removed.
- [x] T011 Run `just capture-fixtures` with a signed-in account (quickstart V1). **Needs the user**: they must sign in with `pass-cli login`. Review the files for personal data, then update the "Unverified" rows in `specs/001-quick-access-launcher/contracts/pass-cli.md` and research.md R4 with the real key names, and note whether plain `item list` includes username, URLs, and a TOTP indicator.
- [x] T012 Create `src/testing.rs` with `FakeRunner` (scripted `(argv matcher → Output)` table, records calls), `FakeBackend` (in-memory vaults/items/fields/TOTP map, scriptable errors and delays via `tokio::time::sleep`), `FakeClipboard` (records `(value, secret: bool)` copies, exposes `last()`), `FakeKeyStore` (in-memory key, can be set to `Unavailable`). Gate with `#[cfg(any(test, feature = "testing"))]`. Traits referenced here are defined in T014, T017, T021; add the fakes incrementally as each trait lands.

### Domain model

- [x] T013 [P] Write failing unit tests in `src/model.rs`: `ItemKind::from_cli("credit-card")` and `("credit_card")` → `CreditCard`; unknown `"passkey-thing"` → `Unknown("passkey-thing")`; `ItemKey` equality and hashing use both `ShareId` and `ItemId`; `ItemSummary::display_title()` returns `"(untitled)"` for an empty title; `FieldRef` with `secret: true` has `Debug` output that does not contain the field value (FieldRef holds no value; assert type has no value field by constructing it).
- [x] T014 Implement `src/model.rs` per data-model.md: newtypes `ShareId`, `ItemId`, `AccountId` (serde transparent); `ItemKey { share: ShareId, item: ItemId }`; `Vault { share_id, name }`; `ItemKind` (`Login | Note | CreditCard | Identity | Alias | SshKey | Wifi | Custom | Unknown(String)`); `ItemSummary { key, vault_name, kind, title, subtitle: Option<String>, urls: Vec<String>, has_totp: bool, totp_fields: Vec<String>, custom_fields: Vec<FieldRef>, modified_at: i64 }`; `FieldRef { name, label, secret: bool }`. All derive `Serialize, Deserialize, Clone, Debug, PartialEq`. Rule from data-model.md: "Never note content or any secret" in `subtitle`.

### `pass-cli` boundary

- [x] T015 [P] Write failing table tests in `src/pass/error.rs` for `classify(exit: Option<i32>, stderr: &str, spawn_err: Option<io::ErrorKind>, timed_out: bool) -> PassError`, covering every row of the "Error mapping" table in contracts/pass-cli.md. Include: stderr with a leading ANSI-coloured tracing line followed by `Error: This operation requires an authenticated client` → `SignedOut`; `Cli { message }` truncated to 200 chars; message never includes stdout.
- [x] T016 Implement `src/pass/error.rs`: `PassError` (`thiserror`) variants `CliMissing`, `SignedOut`, `Locked`, `Network`, `Timeout`, `NotFound`, `Cancelled`, `Protocol { command: &'static str }`, `Cli { message: String }`, and `classify` using the last stderr line that starts with `Error:` (case-insensitive substring matching per contract).
- [x] T017 [P] Write failing integration tests in `tests/pass_cli_integration.rs` for `TokioRunner` using `tests/fixtures/fake-pass-cli`: env `PASS_LOG_LEVEL=off` and `PROTON_PASS_NO_UPDATE_CHECK=1` reach the child (fake echoes env when `FAKE_ECHO_ENV=1`; extend T008 for this); stdin is null; a 1 s timeout on `FAKE_SLEEP=5` returns `Timeout` in < 2 s and the child is gone (`kill -0` fails); dropping the future kills the child; a missing binary returns `CliMissing`; at most 4 children run at once when 10 calls start together (fake logs start/end timestamps).
- [x] T018 Implement `src/pass/runner.rs`: `trait CommandRunner: Send + Sync { async fn run(&self, args: &[String], timeout: Duration, cancel: CancellationToken) -> Result<Output, PassError>; }` and `TokioRunner { bin: PathBuf, sem: Arc<Semaphore> /* 4 permits */ }`. Binary from `COSMIC_PASS_CLI` env or `pass-cli`. Use `tokio::process::Command` with `stdin(null)`, piped stdout/stderr, `kill_on_drop(true)`, `process_group(0)`; on timeout or cancel, `killpg` the group via `rustix`. Stdout is returned as `Zeroizing<Vec<u8>>`.
- [x] T019 [P] Write failing parser tests in `src/pass/parse.rs` using every file in `tests/fixtures/pass-cli/synthetic/` (and `captured/` when present): vaults and items parse; `trashed` items are dropped (FR-010); unknown kind kept; `insta::assert_json_snapshot!` of parsed summaries; **leak test**: `format!("{:?}", parsed)` and `serde_json::to_string(&parsed)` contain no `SECRET-FIXTURE-` substring; `has_totp` true when `content.totp_uri` is non-empty; malformed JSON → `PassError::Protocol`.
- [x] T020 Implement `src/pass/parse.rs`: `parse_vaults(&[u8]) -> Result<Vec<Vault>>`, `parse_items(&[u8], share: &ShareId, vault_name: &str) -> Result<Vec<ItemSummary>>`, `parse_field(&[u8], field: &str) -> Result<SecretString>`, `parse_totp(&[u8]) -> Result<BTreeMap<String, SecretString>>`, `parse_account(&[u8]) -> Result<AccountId>`. Parse through `serde_json::Value` with candidate-key lookup exactly as listed in contracts/pass-cli.md; never copy secret keys into the output; build the `Value` from a zeroizing buffer and drop it before returning.
- [x] T021 [P] Write failing integration tests in `tests/pass_cli_integration.rs` for `PassCli` (real `TokioRunner` + fake binary + synthetic fixtures): `session()` returns `SignedIn(account)` or `SignedOut`; `list_all()` calls `vault list` once and `item list --share-id <id> --filter-state active --output json` once per vault (checked through `FAKE_ARGV_LOG`), running them concurrently; if one vault listing fails, `list_all()` returns the error and no partial list; `get_field(key, "password")` runs `item view pass://S/I/password --output json`; `totp(key)` runs `item totp pass://S/I --output json`; no argv line contains `SECRET-FIXTURE-`.
- [x] T022 Implement `src/pass/backend.rs`: `trait PassBackend: Send + Sync` with `session()`, `list_all() -> Result<(Vec<Vault>, Vec<ItemSummary>)>`, `get_field(&ItemKey, &str, CancellationToken) -> Result<SecretString>`, `totp(&ItemKey, CancellationToken) -> Result<BTreeMap<String, SecretString>>`, `login(line_tx: mpsc::Sender<String>) -> Result<()>`; `PassCli<R: CommandRunner>` implementation using timeouts from contracts/pass-cli.md (`info` 5 s, listings 20 s, field/totp 10 s, login 5 min). If T011 showed that plain `item list` lacks username/URL/TOTP data, add `--show-secrets` here (parser already strips secrets). Leave `login` returning `unimplemented` error variant until T060.

### Search

- [x] T023 [P] Write failing unit tests in `src/core/search.rs`: case-insensitive match; out-of-order fragments (`"hub git"` matches title `GitHub`); a title match outranks a username-only match; URL host and vault name are searchable; two items with the same title in different vaults both appear; results capped at `max_results` (default 50); empty query returns items ordered by `UsageRecord.last_used` desc, then title ascending; trashed items never appear (they are absent from input); result rows carry match ranges for the title.
- [x] T024 Implement `src/core/search.rs`: `SearchIndex::build(&[ItemSummary])` precomputing per-item haystacks (title, subtitle, URL hosts, vault name) as `nucleo_matcher::Utf32String`; `search(&self, query: &str, usage: &UsageTable, max: usize) -> Vec<ResultRow>` using `Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart)`, score = max over fields with title score × 2. `ResultRow { item: usize, score: u32, title_ranges: Vec<Range<usize>> }`.

### Reducer skeleton

- [x] T025 [P] Write failing unit tests in `src/core/state.rs`: `Msg::QueryChanged` recomputes results and resets `selected` to 0; `Msg::SelectNext/Prev/PageDown/PageUp` clamp to bounds; `Msg::Hide` clears `query`, `results` selection, `revealed`, `totp`, `notice`, sets `mode = List`, and returns `Effect::HideWindow`; `Msg::Toggle` alternates show/hide and returns `Effect::ShowWindow` or `Effect::HideWindow`; `Msg::DataLoaded` replaces items atomically, rebuilds the index, and keeps the current query results; `Msg::RefreshRequested` while `refreshing` returns no effect (only one refresh at a time).
- [x] T026 Implement `src/core/state.rs` and `src/core/effects.rs`: `Model { session: SessionState, data: DataState, view: ViewState, prefs: Preferences }` and `Msg` / `Effect` enums per data-model.md, with `fn update(&mut self, msg: Msg, now: i64) -> Vec<Effect>`. No IO, no libcosmic types. `Effect` variants at this point: `ShowWindow`, `HideWindow`, `Refresh`, `Persist`.

### Preferences model

- [x] T027 [P] Write failing unit tests in `src/config.rs`: defaults (`clipboard_clear_secs = 90`, `refresh_stale_secs = 300`, `max_results = 50`, default shortcuts from contracts/keyboard.md); clamping (`clipboard_clear_secs` "10–600", `refresh_stale_secs` "30–86400", `max_results` "10–200"); duplicate chords on load fall back to defaults for the later duplicate.
- [x] T028 Implement `src/config.rs`: `Preferences` with `#[derive(cosmic::cosmic_config::cosmic_config_derive::CosmicConfigEntry)]` (version 1, ID `io.github.ohaukeboe.CosmicPass`), `Action` enum (`copy_primary`, `copy_username`, `copy_totp`, `copy_url`, `open_actions`, `open_detail`, `reveal`, `refresh`, `preferences`), `KeyChord { modifiers: Vec<Modifier>, key: String }`, `Preferences::validated(self) -> Self`.

### App shell

- [x] T029 Implement CLI parsing in `src/main.rs` with `clap` exactly as contracts/cli.md: no args (toggle), `--background`, subcommands `show`, `hide`, `refresh`, and hidden `clipboard-serve --timeout <SECS> [--secret]`. `clipboard-serve` dispatches to `clipboard::serve::main` (stub until T045). Everything else calls `app::run(flags)`. Initialise `tracing_subscriber` writing to stderr at `info` (journald captures it under systemd).
- [x] T030 Implement `src/app/mod.rs`: `CosmicPass` implementing `cosmic::Application` (`APP_ID = "io.github.ohaukeboe.CosmicPass"`), started with `cosmic::app::run_single_instance::<CosmicPass>(Settings::default().no_main_window(true).exit_on_close(false), flags)`. Implement `CosmicFlags` with the JSON action enum `show|hide|refresh`. `dbus_activation`: `Details::Activate` → `Msg::Toggle`, `ActivateAction` → mapped message. `update` forwards to `Model::update` and executes returned `Effect`s through `fn run_effect(&mut self, Effect) -> Task<Message>`; IO effects use `cosmic::task::future`. On init: if not `--background`, emit `Msg::Toggle`; always emit `Msg::RefreshRequested`. `Effect::Refresh` → `backend.list_all()` → `Msg::DataLoaded` or `Msg::RefreshFailed(PassError)`.
- [x] T031 Implement `src/app/surface.rs` per research R2: `show()` creates a layer surface via `cosmic::surface::surface_task(app_layer_shell(...))` with `KeyboardInteractivity::Exclusive`, `Anchor::TOP`, top margin 20% of output height, width 640 px, `exclusive_zone: -1`, namespace `cosmic-pass`, then focuses the search input (`text_input::focus`); `hide()` calls `destroy_layer_surface`. Subscribe with `listen_raw` to `wayland::Event::Layer(LayerEvent::Unfocused, ..)` → `Msg::Hide`. Ignore `Msg::Toggle` within 100 ms after a hide (debounce).
- [x] T032 [P] Write failing unit tests in `src/app/keys.rs` for `map_key(key, modifiers, mode, prefs, caret_at_end) -> Option<Msg>` covering list-mode navigation rows in contracts/keyboard.md (`Down`/`Ctrl+N`/`Ctrl+J`, `Up`/`Ctrl+P`/`Ctrl+K`, `Page Down`/`Page Up`, `Escape`, `F5`); then implement `src/app/keys.rs` and subscribe to key presses in `src/app/mod.rs` via `listen_raw`.
- [x] T033 Implement `src/app/view/list.rs` and `src/app/view/mod.rs`: search `text_input` with id `search`; up to `max_results` rows, each showing item-kind icon (freedesktop symbolic names: `dialog-password-symbolic` login, `x-office-document-symbolic` note, `auth-smartcard-symbolic` card, `contact-new-symbolic` identity, `mail-forward-symbolic` alias, `network-wireless-symbolic` Wi-Fi, `utilities-terminal-symbolic` SSH key, `emblem-documents-symbolic` other), title with matched ranges bold, subtitle, vault name right-aligned (FR-008); selected row highlighted and kept visible with `scrollable::snap_to`; "No items found" when the query has no results; a small spinner in the header while `refreshing`.

**Checkpoint**: `COSMIC_PASS_CLI=tests/fixtures/fake-pass-cli FAKE_FIXTURE_DIR=tests/fixtures/pass-cli/synthetic just run` opens a centered popup listing fixture items, filtering on each keystroke; Escape and focus loss hide it; running `cosmic-pass` again toggles it.

---

## Phase 3: User Story 1 - Find a login and copy its password (Priority: P1) 🎯 MVP

**Goal**: Shortcut → type → Enter copies the primary secret; the clipboard clears itself.

**Independent Test**: With a signed-in account, press the shortcut, type part of a title,
press Enter, paste: the value equals the password. After 90 s the clipboard is empty
(quickstart V2, V4).

### Tests for User Story 1 ⚠️ (write first, see them fail)

- [x] T034 [P] [US1] Write failing unit tests in `src/core/actions.rs` for `primary_action(&ItemSummary) -> Option<CopySource>` using the data-model.md table verbatim: Login → `password`; CreditCard → card `number`; Note → `note`; Alias → alias email; Identity, SshKey, Wifi, Custom → "first secret custom/standard field; else first field"; `Unknown` → first secret custom field or `None`.
- [x] T035 [P] [US1] Write failing unit tests in `src/core/usage.rs`: recording a use sets `last_used` and increments `count` (saturating at `u32::MAX`); "Max 200 records; oldest evicted"; `prune(&items)` removes records whose item no longer exists; records hold only `ItemKey`, `last_used`, `count`.
- [x] T036 [P] [US1] Write failing unit tests in `src/clipboard/mod.rs` for `ClipboardJobs` using a fake helper command (`tests/fixtures/fake-clipboard-serve`, a `sh` script that reads stdin to `$FAKE_CLIP_OUT` and then sleeps `$FAKE_CLIP_HOLD` seconds or exits at once if `FAKE_CLIP_LOSE=1`), with `tokio::time::pause`: secret copy kills the helper when `clipboard_clear_secs` expires (within 1 s, SC-005); helper exiting early (ownership lost) ends the job without further action; a new copy kills the previous helper first; non-secret copy starts no timeout; the value reaches the helper through stdin only (argv log has no value). Create the fake script as part of this task.
- [x] T037 [P] [US1] Write failing unit tests in `src/clipboard/serve.rs` for the pure parts: `offers(secret: bool)` returns MIME list `text/plain;charset=utf-8`, `text/plain`, `UTF8_STRING` plus `x-kde-passwordManagerHint` = `secret` only when `secret` is true (contracts/cli.md); `read_value(stdin)` returns exit code 3 on empty input.
- [x] T038 [US1] Write failing acceptance tests in `tests/story1_copy_password.rs` driving `Model::update` plus an `EffectRunner` test harness (`src/testing.rs`, executes effects against `FakeBackend` and `FakeClipboard`, feeding results back as messages): (1) toggle shows window with empty query; (2) typing `git` lists items whose title, username, URL, or vault contains it, best title match first; (3) Enter on a login copies its password with `secret = true`, hides the window, clears the query, and records usage; (4) Escape hides and clears the query; (5) Enter with no results does nothing; (6) Enter on a slow fetch then Escape cancels it and nothing is copied even after the fetch would have completed; (7) backend returns `NotFound` → notice "Item no longer exists" and `Effect::Refresh`; (8) empty query after a copy lists the copied item first (FR-009).

### Implementation for User Story 1

- [x] T039 [P] [US1] Implement `src/core/actions.rs`: `CopySource = Field(FieldRef) | Totp { field: String } | Url(String)`, `CopyAction { key: ItemKey, source: CopySource }`, and `primary_action` per T034.
- [x] T040 [P] [US1] Implement `src/core/usage.rs`: `UsageTable` (`HashMap<ItemKey, UsageRecord>`), `record(key, now)`, `prune(&[ItemSummary])`, `recency(&ItemKey) -> Option<i64>`, eviction at 200 entries.
- [x] T041 [US1] Extend `src/core/state.rs` and `src/core/effects.rs`: `Msg::CopyPrimary` → if a row is selected and `primary_action` exists, set `view.pending = PendingFetch { key, action, cancel }` and return `Effect::FetchAndCopy { key, field, secret, cancel }`; `Msg::CopyFetched { key, result }` → ignore if `pending` is cleared or its key differs, else on success return `Effect::Copy { value, secret }`, record usage, return `Effect::Persist` and `Msg::Hide` effects; on `NotFound` set notice "Item no longer exists" and return `Effect::Refresh`; on other errors set notice with the error text. `Msg::Escape` with `pending` set → cancel token, clear `pending`, keep window open; without `pending` → hide. Wire `UsageTable` into search (empty-query ordering).
- [x] T042 [US1] Implement the `EffectRunner` harness in `src/testing.rs` used by T038 and make T038 pass.
- [x] T043 [US1] Implement `src/clipboard/mod.rs`: `trait Clipboard: Send + Sync { async fn copy(&self, value: SecretString, secret: bool) -> Result<(), ClipboardError>; }`; `HelperClipboard` spawns `std::env::current_exe()` with `clipboard-serve --timeout <secs> [--secret]` (helper binary path overridable via `COSMIC_PASS_CLIPBOARD_HELPER` for tests), writes the value to stdin, closes stdin, drops the secret; keeps one `ClipboardJob { child, expires_at, secret }`; kills any previous job first; a background task waits for `child.wait()` or the deadline, and on deadline kills the child. `ClipboardError::Unavailable` when the helper exits with code 2.
- [x] T044 [US1] Make T036 pass; adjust `HelperClipboard` only through its public API.
- [x] T045 [US1] Implement `src/clipboard/serve.rs::main(timeout, secret)`: read stdin into `Zeroizing<Vec<u8>>` (exit 3 if empty); build `wl_clipboard_rs::copy::Options` with `foreground(true)`, `serve_requests(ServeRequests::Unlimited)`, `clipboard(ClipboardType::Regular)`; call `copy_multi` with `MimeSource`s from `offers(secret)`; exit 0 when `copy_multi` returns (selection replaced); exit 2 if neither `ext-data-control` nor `wlr-data-control` is available. Enforce `--timeout` as a self-destruct safety net (exit after timeout + 5 s even if the parent died), using a watchdog thread.
- [x] T046 [US1] Wire US1 into the app in `src/app/mod.rs` and `src/app/keys.rs`: `Enter` (configurable `copy_primary`) → `Msg::CopyPrimary`; `Escape` → `Msg::Escape`; double-click on a row → select + `Msg::CopyPrimary`; `Effect::FetchAndCopy` → `backend.get_field(..)` in `cosmic::task::future` → `Msg::CopyFetched`; `Effect::Copy` → `HelperClipboard::copy` (errors → notice "Clipboard unavailable"). Show a spinner on the pending row in `src/app/view/list.rs`. Show `notice` under the search field for 3 s (`Msg::NoticeExpired` via `cosmic::task::future(sleep)`).
- [x] T047 [P] [US1] Create `data/io.github.ohaukeboe.CosmicPass.desktop` (`Exec=cosmic-pass`, `Type=Application`, `Categories=Utility;Security;`, `Keywords=password;proton;pass;`, `Icon=io.github.ohaukeboe.CosmicPass`, `X-CosmicApplet=false`) and `data/cosmic-pass.service` (`[Unit] PartOf=graphical-session.target After=graphical-session.target`, `[Service] ExecStart=%h/.local/bin/cosmic-pass --background Restart=on-failure`, `[Install] WantedBy=graphical-session.target`).
- [x] T048 [P] [US1] Create `data/icons/io.github.ohaukeboe.CosmicPass.svg` (simple original key-and-lightning glyph; no Proton branding) and `data/io.github.ohaukeboe.CosmicPass.metainfo.xml` (AppStream: id, name "COSMIC Pass", summary, `project_license`, description noting it is an unofficial front-end for `pass-cli`).
- [x] T049 [US1] Implement `just install-user` in `justfile`: `cargo build --release`; install binary to `~/.local/bin/cosmic-pass`, desktop file, metainfo, and icon under `~/.local/share/`, and the unit to `~/.config/systemd/user/cosmic-pass.service`; print the `systemctl --user enable --now cosmic-pass.service` command and the COSMIC Settings shortcut instructions from quickstart.md.
- [x] T050 [US1] Write `README.md`: what it is, requirements (`pass-cli` signed in, COSMIC, Secret Service), install (`just install-user`), shortcut setup (Settings → Keyboard → Custom shortcut → `cosmic-pass`, suggested `Super+Shift+P`), default keys (link contracts/keyboard.md), security notes (secrets fetched on demand, clipboard cleared after timeout, COSMIC clipboard manager ignores `x-kde-passwordManagerHint` today), and an "unofficial, not affiliated with Proton" statement.

**Checkpoint**: MVP. T038 passes; quickstart V2 and V4 pass on a real COSMIC session.

---

## Phase 4: User Story 2 - Copy other fields and one-time codes (Priority: P2)

**Goal**: Dedicated shortcuts and an action list copy username, TOTP, website, and any field.

**Independent Test**: Highlight a login with TOTP, press `Ctrl+O`, paste: matches the code in
Proton Pass (quickstart V3).

### Tests for User Story 2 ⚠️

- [x] T051 [P] [US2] Write failing unit tests in `src/core/actions.rs` for `all_actions(&ItemSummary) -> Vec<(CopyAction, Option<Action>)>` using the data-model.md table: Login → `password`, `username`, `email`, `totp` (one per `totp_fields` entry), `url` (first), each custom field; CreditCard → holder name, expiry, `cvv`, custom fields (plus `number` as primary); Note → `note`, custom fields; Alias → alias email, `note`; each action carries its `Action` shortcut if one exists; secret flags match `FieldRef.secret`.
- [x] T052 [P] [US2] Write failing unit tests in `src/app/keys.rs` for the list-mode copy chords (`Ctrl+U`, `Ctrl+O`, `Ctrl+L`, `Tab`, `Right` only when the caret is at the end of the query) and action-list mode keys (`Up`/`Down`, `Enter`, `Escape`/`Left`/`Shift+Tab` go back) from contracts/keyboard.md, including rebinding through `Preferences.shortcuts`.
- [x] T053 [US2] Write failing acceptance tests in `tests/story2_other_fields.rs` with the `EffectRunner` harness: (1) `Msg::CopyUsername` copies the username with `secret = false` and hides; falls back to email when username is empty; (2) `Msg::CopyTotp` on a TOTP login copies the code from `FakeBackend::totp` with `secret = true`; (3) `Msg::CopyTotp` on a login without TOTP sets notice "This item has no one-time code", no copy, window stays open; (4) `Msg::CopyUrl` copies the first URL without any backend call and with `secret = false`; (5) `Msg::OpenActions` lists all fields from `all_actions` in `ViewState.mode = Actions(key)`; Enter there copies the highlighted field; Escape returns to `List` with the same selection; (6) item with two TOTP fields lists both.

### Implementation for User Story 2

- [x] T054 [US2] Implement `all_actions` in `src/core/actions.rs` (T051).
- [x] T055 [US2] Extend `src/core/state.rs`: `Msg::CopyUsername`, `Msg::CopyTotp`, `Msg::CopyUrl`, `Msg::OpenActions`, `Msg::ActionsSelectNext/Prev`, `Msg::ActionsActivate`, `Msg::Back`; `Effect::FetchTotpAndCopy { key, field, cancel }`; `Url` source emits `Effect::Copy` directly; non-secret copies use `secret = false` (no clipboard timeout, per data-model.md `ClipboardJob`). Make T053 pass.
- [x] T056 [US2] Implement the `FetchTotpAndCopy` effect in `src/app/mod.rs` using `backend.totp(..)` and selecting the requested field (or the only one); implement the chords in `src/app/keys.rs` (T052).
- [x] T057 [US2] Implement `src/app/view/actions.rs`: list of `label — shortcut` rows for the selected item, secret values never shown (labels only), highlighted row, header with item title and vault.

**Checkpoint**: US1 and US2 work independently; quickstart V3 passes.

---

## Phase 5: User Story 4 - Recover from a signed-out or unavailable state (Priority: P2)

**Goal**: Clear status and a sign-in action instead of an empty list.

**Independent Test**: `pass-cli logout`, open the window: "Not signed in" and a "Sign in"
action appear (quickstart V5).

### Tests for User Story 4 ⚠️

- [x] T058 [P] [US4] Write failing unit tests in `src/core/state.rs` for the `SessionState` machine in data-model.md: every transition in the diagram, including `SignedIn` + refresh error `SignedOut` → `SignedOut` with `Effect::DeleteCache`; `info` returning a different account → `Effect::DeleteCache` + `Effect::Refresh`; network error leaves state unchanged and sets `data.stale = true`; `LoggingIn` + success → `Checking` + `Effect::ProbeSession`.
- [x] T059 [US4] Write failing acceptance tests in `tests/story4_status.rs` with the `EffectRunner` harness: (1) backend `SignedOut` on open → `view` shows status `NotSignedIn` and `Msg::StartLogin` is available; (2) `CliMissing` → status names `pass-cli` and the install URL `https://protonpass.github.io/pass-cli/`; (3) refresh fails with `Network` while items exist → items stay searchable, `stale = true`; (4) one vault fails during `list_all` → old list kept, `stale = true`; (5) `Msg::StartLogin` streams a URL line from `FakeBackend::login` into `view.login_url`; success triggers a refresh; (6) window open with `fetched_at` older than `refresh_stale_secs` emits `Effect::Refresh`; newer does not; (7) `F5` emits `Effect::Refresh`.

### Implementation for User Story 4

- [x] T060 [US4] Implement `PassCli::login` in `src/pass/backend.rs`: spawn `pass-cli login` (stdin null, no `--output`), forward each stdout and stderr line to `line_tx`, 5-minute timeout, success on exit 0. Add a fake-binary integration test in `tests/pass_cli_integration.rs` where the fake prints `Please go to https://example.invalid/login` and exits 0.
- [x] T061 [US4] Extend `src/core/state.rs` and `src/core/effects.rs` with `SessionState` transitions, `Msg::SessionProbed`, `Msg::RefreshFailed`, `Msg::StartLogin`, `Msg::LoginLine(String)` (extract the first `https://` URL), `Msg::LoginFinished`, the stale-on-open rule (`refresh_stale_secs`), and effects `ProbeSession`, `StartLogin`, `DeleteCache`. Make T058 and T059 pass.
- [x] T062 [US4] Implement `src/app/view/status.rs`: full-panel messages for `NotSignedIn` (button "Sign in", then "Waiting for browser sign-in…" with the URL as a clickable link opened through `cosmic::desktop` / `open::that`), `Locked` ("Session locked — run `pass-cli session unlock` in a terminal"), `CliMissing` (names `pass-cli` and the install URL), `Error { message }`; a compact "Data may be out of date" indicator in the list header when `stale`. Wire `ProbeSession`, `StartLogin` (subscription streaming `LoginLine`), and `F5` in `src/app/mod.rs`.

**Checkpoint**: US1, US2, US4 work; quickstart V5 passes.

---

## Phase 6: User Story 3 - View item details without leaving the keyboard (Priority: P3)

**Goal**: A detail pane with masked secrets, reveal, and a live TOTP countdown.

**Independent Test**: `Ctrl+I` on an item shows its fields with the password masked;
`Ctrl+R` reveals; the TOTP code refreshes when its period ends (quickstart V6).

### Tests for User Story 3 ⚠️

- [x] T063 [P] [US3] Write failing unit tests in `src/core/totp.rs`: with `period = 30`, `valid_until(now)` is the next multiple of 30 strictly after `now`; `remaining(now)` counts down 30…1; `needs_refresh(now)` is true when `now >= valid_until`.
- [x] T064 [US3] Write failing acceptance tests in `tests/story3_detail_pane.rs` with the `EffectRunner` harness: (1) `Msg::OpenDetail` sets `mode = Detail(key)`, shows non-secret fields, `revealed = None`, and emits `Effect::FetchTotp` only when `has_totp`; (2) `Msg::ToggleReveal` emits `Effect::FetchReveal`; the result sets `revealed`; a second toggle clears it; (3) `Msg::Back` or `Msg::Hide` drops `revealed` and `totp`; (4) `Msg::Tick(now)` past `valid_until` emits `Effect::FetchTotp` again; (5) copy chords still work inside the detail pane.

### Implementation for User Story 3

- [x] T065 [P] [US3] Implement `src/core/totp.rs` (`TotpDisplay { field, code: SecretString, period: u32, valid_until: i64 }`, period fixed at 30 per research R4).
- [x] T066 [US3] Extend `src/core/state.rs`: `Msg::OpenDetail`, `Msg::ToggleReveal`, `Msg::RevealFetched`, `Msg::TotpFetched`, `Msg::Tick(i64)`; effects `FetchReveal { key, field, cancel }`, `FetchTotp { key, cancel }`. Make T064 pass.
- [x] T067 [US3] Implement `src/app/view/detail.rs` (title, vault, kind, username, websites, password shown as `••••••••` unless revealed, TOTP code with a countdown progress bar) and wire `Ctrl+I`/`Ctrl+R` in `src/app/keys.rs`; add a 1 s `cosmic::iced::time::every` subscription active only while `mode == Detail` and `totp` is set.

**Checkpoint**: All four stories work independently; quickstart V6 passes.

---

## Phase 7: Encrypted metadata cache (cross-cutting: FR-023, FR-024, FR-024a, FR-024b, FR-026)

**Purpose**: Instant results after login or reboot without exposing metadata.

### Tests ⚠️

- [x] T068 [P] Write failing unit tests in `src/cache/crypto.rs`: seal/open round-trip; header is magic `CPC1` + version `1` + 24-byte nonce (contracts/cache-format.md); two seals of the same plaintext differ; flipping any header or ciphertext byte fails to open; wrong key fails; truncated file fails.
- [x] T069 [P] Write failing integration tests in `tests/cache_integration.rs` (temp dir via `COSMIC_PASS_CACHE_DIR`, `FakeKeyStore`): save then load returns the same `CacheFile`; directory mode `0700` and file mode `0600`; a crash between temp write and rename leaves the old file intact (simulate by leaving a stray temp file); keyring `Unavailable` → `save` writes nothing and `load` returns `None` (FR-024a); `COSMIC_PASS_NO_KEYRING=1` behaves the same; corrupt file → deleted and `None`; `account` mismatch → deleted and `None`; `delete_all()` removes the file and the keyring item; **leak test**: a `CacheFile` built by parsing the synthetic fixtures, saved and decrypted, contains no `SECRET-FIXTURE-` bytes (SC-006); saves within 2 s are debounced into one write.
- [x] T070 Write failing acceptance tests in `tests/story1_copy_password.rs` (append) for startup with cache: `Msg::CacheLoaded` fills items with `stale = true` and `source = DiskCache` before `DataLoaded` arrives; typing works immediately (FR-023).

### Implementation

- [x] T071 [P] Implement `src/cache/crypto.rs` with `chacha20poly1305::XChaCha20Poly1305`, AAD = header bytes 0..30, random nonce from `rand::rngs::OsRng`.
- [x] T072 [P] Implement `src/cache/keystore.rs`: `trait KeyStore: Send + Sync { async fn get_or_create(&self) -> Result<Option<SecretBox<[u8; 32]>>>; async fn delete(&self) -> Result<()>; }`; `Oo7KeyStore` using `oo7::Keyring::new()` with attributes `application=io.github.ohaukeboe.CosmicPass`, `purpose=cache-key`, label `COSMIC Pass cache key`; returns `Ok(None)` if the keyring is locked or unavailable or `COSMIC_PASS_NO_KEYRING=1`.
- [x] T073 Implement `src/cache/store.rs`: `CacheStore { dir, keystore }` with `load(account) -> Option<CacheFile>`, `save(&CacheFile)` (atomic: `tempfile::NamedTempFile::new_in(dir)`, permissions `0600`, `sync_all`, `persist`, fsync dir; debounce 2 s), `delete_all()`; every rule row in contracts/cache-format.md. Encoding with `postcard`. Make T068 and T069 pass.
- [x] T074 Wire the cache into `src/core/state.rs` and `src/app/mod.rs`: on startup `Effect::LoadCache` → `Msg::CacheLoaded`; `Effect::Persist` after `DataLoaded` and after usage updates builds a `CacheFile` (items, vaults, usage, account, `fetched_at`); `Effect::DeleteCache` → `store.delete_all()`; usage table restored from cache and pruned on refresh. Make T070 pass.

**Checkpoint**: quickstart V7 passes.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [x] T075 [P] Write failing tests, then implement `src/app/view/preferences.rs` and its reducer messages in `src/core/state.rs` (FR-027): edit `clipboard_clear_secs` (spin box, "10–600") and rebind each `Action` by pressing a chord; reject a duplicate chord with an inline error; save through `cosmic_config`; `Ctrl+,` opens it; config file changes apply live through `cosmic_config::config_subscription`.
- [x] T076 [P] Write `tests/search_bench.rs` (`#[ignore]`, run by `just bench`): build 5,000 generated `ItemSummary`s, replay typing `"github personal"` one character at a time 100 times, assert p95 per-keystroke `search()` time < 50 ms (SC-002) and p95 `Model::update(QueryChanged)` < 50 ms.
- [x] T077 [P] Add a responsiveness test in `tests/story1_copy_password.rs`: with `FakeBackend` delaying `list_all` 3 s, 200 `QueryChanged` messages processed during the delay each return in < 5 ms and all are reflected in `view.query` (SC-004).
- [x] T078 [P] Add a test in `tests/pass_cli_integration.rs` with `FAKE_ITEMS=5000` confirming `list_all` parses 5,000 items and search over them works.
- [x] T079 Create `scripts/leak-scan.sh` (called by `just leak-scan`): run the full test suite with `COSMIC_PASS_CACHE_DIR` and `XDG_CONFIG_HOME` pointed at a temp dir and with stderr captured; grep the temp dirs (decrypted cache is covered by T069) and captured logs for `SECRET-FIXTURE-`; fail on any match (SC-006).
- [x] T080 Grep-based review, fix findings: no `#[derive(Debug)]` on any type holding `SecretString` without a manual redacting impl; no `tracing` macro takes a secret; no `.expose_secret()` outside `clipboard/mod.rs`, `clipboard/serve.rs`, and `app/view/detail.rs`; `unsafe_code` forbidden. Record results in the PR description.
- [x] T081 Evaluate `iced_test` (research V2): add `iced_test` as a dev-dependency from pop-os/iced at the rev of libcosmic's `iced` submodule; try one simulator test typing into the search field and pressing Enter in `tests/ui_smoke.rs`. If it works, keep it; if not, delete the test, remove the dependency, and record the reason in `specs/001-quick-access-launcher/research.md` R10.
- [x] T082 Run `just check` and `just bench` (recipes in `justfile`); fix everything until they pass, with changed-line coverage ≥ 80%.
- [ ] T083 Run quickstart.md V1–V9 on a real COSMIC session (**needs the user**). Record results and any contract corrections (e.g. locked-session error text, whether killing the helper clears the selection) in `specs/001-quick-access-launcher/research.md` "Open validation items", and file `bd` issues for failures.
- [x] T084 Remove the resolved `TODO(TECH_STACK)` from `.specify/memory/constitution.md` by amending it through `/speckit-constitution` (PATCH bump) now that tools are named in `CLAUDE.md`/`AGENTS.md`.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none. T005 depends on T002.
- **Foundational (Phase 2)**: depends on Setup. Blocks every story.
  - T011 (real fixtures) needs the user and a signed-in `pass-cli`; T022's `--show-secrets`
    decision depends on it. Everything else can proceed with synthetic fixtures.
- **US1 (Phase 3)**: depends on Phase 2.
- **US2 (Phase 4)**: depends on Phase 2; reuses `actions.rs`, `state.rs`, and the clipboard
  from US1 (T039, T041, T043). Test in isolation via its own acceptance file.
- **US4 (Phase 5)**: depends on Phase 2 only. Can run in parallel with US1 if different people
  own `state.rs` sections; otherwise do after US1.
- **US3 (Phase 6)**: depends on Phase 2; copy chords inside the pane depend on US1/US2.
- **Cache (Phase 7)**: depends on US1 (usage table) and US4 (`DeleteCache` effect).
- **Polish (Phase 8)**: depends on the stories it touches.

### Within Each Phase

- Test task → see it fail → implementation task.
- `model.rs` → `pass/*` → `core/*` → `app/*`.
- `src/core/state.rs` is touched by many tasks: do those tasks sequentially.

### Story completion order

```text
Setup → Foundational → US1 (MVP) → US2 → US4 → US3 → Cache → Polish
```

---

## Parallel Examples

### Phase 2

```text
T008 fake-pass-cli script      T009 synthetic fixtures      T010 capture script
T013 model tests               T015 error tests             T023 search tests
T027 config tests
```

### User Story 1

```text
T034 actions tests   T035 usage tests   T036 clipboard job tests   T037 serve tests
then: T039 actions impl   T040 usage impl   T047 desktop/service files   T048 icon/metainfo
```

### User Story 2

```text
T051 all_actions tests   T052 key chord tests
```

### Phase 7

```text
T068 crypto tests   T069 store tests
then: T071 crypto impl   T072 keystore impl
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Phase 1 → Phase 2 (ask the user to sign in for T011 early).
2. Phase 3 (US1).
3. **Stop and validate**: `just check`, then quickstart V2 and V4 on COSMIC.
4. Install with `just install-user` and use it daily.

### Incremental Delivery

1. US2: field and TOTP shortcuts (quickstart V3).
2. US4: status and sign-in (quickstart V5).
3. US3: detail pane (quickstart V6).
4. Cache: instant results after reboot (quickstart V7).
5. Polish: preferences, benchmark, leak scan, full quickstart.

---

## Notes

- Track execution in `bd`: run `/speckit-taskstoissues` or create one `bd` issue per phase
  with tasks as children before starting.
- Commit after each task or logical group, using Conventional Commits, only with user approval.
- Tasks marked **needs the user**: T011, T083.

---

## Phase 9: Convergence

**Purpose**: Close gaps found by `/speckit-converge` between the artifacts and the code.

- [ ] T085 Bold the matched characters of result titles in `src/app/view/list.rs` using the `title_indices` already returned by `src/core/search.rs` (rich text spans) per tasks T033 / FR-008 (partial)
- [ ] T086 Offer every website of an item in the action list: keep all URLs as fields in `src/pass/parse.rs` (`url`, `url2`, … or one entry per URL) and list them in `all_actions` in `src/core/actions.rs` per spec Edge Cases "Item has multiple websites or multiple TOTP fields" (partial)
- [ ] T087 Report a network failure distinctly: track the last refresh error in `src/core/state.rs` and show "Can't reach Proton Pass — showing saved items" in the stale indicator in `src/app/view/list.rs`, with a reducer test per FR-020 (partial)
- [x] T088 Add a `LICENSE` file matching the `GPL-3.0-only` declared in `Cargo.toml` and `data/io.github.ohaukeboe.CosmicPass.metainfo.xml`, or change both declarations to the license the maintainer chooses per Constitution V (missing)
- [ ] T089 Measure window-open latency for SC-001: log the elapsed time from `Msg::Toggle`/`dbus_activation` to the layer surface being ready in `src/app/mod.rs` at debug level, and record the measured p95 in `specs/001-quick-access-launcher/research.md` during quickstart V2 per SC-001 (missing)
- [ ] T090 Derive the popup top margin from the active output height (~20%) instead of the fixed `TOP_MARGIN` in `src/app/surface.rs`, or amend plan R2 to state the fixed offset per plan R2 / tasks T031 (partial)
- [x] T091 Decide the secondary line for note items in `src/pass/parse.rs`: either a non-secret preview source or a recorded decision in `specs/001-quick-access-launcher/data-model.md` that notes have none per FR-008 (partial)
