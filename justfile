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
cov:
    #!/usr/bin/env bash
    set -euo pipefail
    report=$(cargo llvm-cov nextest --all-features --no-tests=pass \
        --ignore-filename-regex 'src/main\.rs' --summary-only --json)
    read -r count percent < <(jq -r '.data[0].totals.lines | "\(.count) \(.percent)"' <<<"$report")
    echo "line coverage: ${percent}% of ${count} lines"
    if [ "$count" -gt 0 ] && ! awk -v p="$percent" 'BEGIN { exit !(p >= 80) }'; then
        echo "coverage below 80%" >&2
        exit 1
    fi

# All pre-merge gates.
check: fmt lint test cov

# Run the app.
run *ARGS:
    cargo run -- {{ARGS}}

# Search benchmark (SC-002).
bench:
    cargo nextest run --release --all-features --run-ignored only --no-tests=pass search_bench

# Install binary, desktop entry, and user service into the home directory.
install-user:
    @echo "install-user: not implemented yet (T049)" && exit 1

# Capture redacted pass-cli output as test fixtures (needs a signed-in pass-cli).
capture-fixtures:
    @echo "capture-fixtures: not implemented yet (T010)" && exit 1

# Scan files written during tests for leaked secrets (SC-006).
leak-scan:
    @echo "leak-scan: not implemented yet (T079)" && exit 1
