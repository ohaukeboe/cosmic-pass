# Quality gates and developer tasks. Run inside `nix-shell` (or with direnv).

default: check

# Check formatting.
fmt:
    cargo fmt --all --check

# Lint with warnings as errors.
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run the test suite.
test:
    cargo nextest run --all-features --no-tests=pass

# Run tests with coverage; fails below 80% line coverage (skipped while no lines are coverable).
# Excluded: the binary entry point and the libcosmic window/view glue, which need a live
# compositor and are covered by the manual quickstart checks (plan.md, Complexity Tracking #1).
cov:
    #!/usr/bin/env bash
    set -euo pipefail
    report=$(cargo llvm-cov nextest --all-features --no-tests=pass \
        --ignore-filename-regex 'src/(main\.rs|app/mod\.rs|app/surface\.rs|app/view/)' --summary-only --json)
    read -r count percent < <(jq -r '.data[0].totals.lines | "\(.count) \(.percent)"' <<<"$report")
    echo "line coverage: ${percent}% of ${count} lines"
    if [ "$count" -gt 0 ] && ! awk -v p="$percent" 'BEGIN { exit !(p >= 80) }'; then
        echo "coverage below 80%" >&2
        exit 1
    fi

# All pre-merge gates.
check: fmt lint test cov

# Run the app (release: debug builds render far too slowly to use).
run *ARGS:
    cargo run --release -- {{ARGS}}

# Run an unoptimized build (slow rendering; for backtraces only).
run-debug *ARGS:
    cargo run -- {{ARGS}}

# Search benchmark (SC-002).
bench:
    cargo nextest run --release --all-features --run-ignored only --no-tests=pass search_bench

# Install binary, desktop entry, and user service into the home directory.
install-user:
    cargo build --release
    install -Dm755 target/release/cosmic-pass ~/.local/bin/cosmic-pass
    install -Dm644 data/io.github.ohaukeboe.CosmicPass.desktop ~/.local/share/applications/io.github.ohaukeboe.CosmicPass.desktop
    install -Dm644 data/io.github.ohaukeboe.CosmicPass.metainfo.xml ~/.local/share/metainfo/io.github.ohaukeboe.CosmicPass.metainfo.xml
    install -Dm644 data/icons/io.github.ohaukeboe.CosmicPass.svg ~/.local/share/icons/hicolor/scalable/apps/io.github.ohaukeboe.CosmicPass.svg
    install -Dm644 data/cosmic-pass.service ~/.config/systemd/user/cosmic-pass.service
    @echo
    @echo "Installed. Next steps:"
    @echo "  systemctl --user daemon-reload"
    @echo "  systemctl --user enable --now cosmic-pass.service"
    @echo "Then add a shortcut: COSMIC Settings > Keyboard > Keyboard Shortcuts > Custom,"
    @echo "command 'cosmic-pass', keys Super+Shift+P."

# Capture redacted pass-cli output as test fixtures (needs a signed-in pass-cli).
capture-fixtures:
    scripts/capture-fixtures.sh

# Scan files written during tests for leaked secrets (SC-006).
leak-scan:
    scripts/leak-scan.sh
