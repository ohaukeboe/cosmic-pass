#!/usr/bin/env bash
# Scans everything the test suite and a short app run write for fixture secrets (SC-006).
#
# Fixture secrets all contain the marker SECRET-FIXTURE- (or SECRETFIXTURE inside TOTP URIs).
# The fixture files themselves are excluded; only files written during the run and the
# captured logs are scanned.
set -euo pipefail

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export COSMIC_PASS_CACHE_DIR="$work/cache"
export XDG_CONFIG_HOME="$work/config"
export XDG_CACHE_HOME="$work/xdg-cache"
export TMPDIR="$work/tmp"
# The encrypted cache cannot be grepped, and the run must not touch the real keyring.
export COSMIC_PASS_NO_KEYRING=1
mkdir -p "$TMPDIR"
root=$(pwd)

echo "==> test suite"
cargo nextest run --all-features --no-tests=pass --no-capture >"$work/tests.log" 2>&1

echo "==> app run against the fake pass-cli"
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    COSMIC_PASS_CLI="$root/tests/fixtures/fake-pass-cli" \
        FAKE_FIXTURE_DIR="$root/tests/fixtures/pass-cli/synthetic" \
        COSMIC_SINGLE_INSTANCE=0 \
        timeout 5 cargo run --quiet -- --background >"$work/app.log" 2>&1 || true
else
    echo "(skipped: no Wayland session)"
fi

echo "==> scanning"
if grep -rIl -e 'SECRET-FIXTURE-' -e 'SECRETFIXTURE' "$work"; then
    echo "LEAK: fixture secrets found in the files above" >&2
    exit 1
fi
echo "no leaks found"
