#!/usr/bin/env bash
# Scans everything the test suite and a short app run write for fixture secrets (SC-006).
#
# Fixture secrets all contain the marker SECRET-FIXTURE- (or SECRETFIXTURE inside TOTP URIs).
# The fixture files themselves are excluded; only files written during the run and the
# captured logs are scanned.
#
# Exit codes: 0 = every step ran and nothing leaked; 1 = a fixture secret was found;
# 2 = a step could not run, so the scan proves nothing about it.
#
# The live pass-cli suite is scanned differently: a real account's secrets carry no marker, so
# a marker grep would prove nothing about them. Instead its output is checked against the fixed
# vocabulary it is allowed to print. Set COSMIC_PASS_LIVE=1 to include that step; it needs a
# signed-in pass-cli and is read-only.
#
# The app run uses the real keyring so that an encrypted cache is actually written and
# scanned; it creates or reuses the app's own "COSMIC Pass cache key" item and writes the
# cache into a temporary directory. Export COSMIC_PASS_NO_KEYRING=1 beforehand to opt out —
# the cache is then reported as a skipped step rather than a pass.
set -euo pipefail

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export COSMIC_PASS_CACHE_DIR="$work/cache"
export XDG_CONFIG_HOME="$work/config"
export XDG_CACHE_HOME="$work/xdg-cache"
export TMPDIR="$work/tmp"
mkdir -p "$TMPDIR"
root=$(pwd)
skipped=()

echo "==> test suite"
COSMIC_PASS_NO_KEYRING=1 \
    cargo nextest run --all-features --no-tests=pass --no-capture >"$work/tests.log" 2>&1

echo "==> app run against the fake pass-cli"
if [ -z "${WAYLAND_DISPLAY:-}" ]; then
    skipped+=("app run: no Wayland session")
elif [ "${COSMIC_PASS_NO_KEYRING:-}" = 1 ]; then
    skipped+=("encrypted cache: COSMIC_PASS_NO_KEYRING=1, so no cache file is written")
else
    # Built before the clock starts. `cargo run` inside the timeout spends most of it
    # linking — the suite above builds with --all-features, so the default-feature binary
    # is usually stale — and the app would be killed before the cache write, which is
    # debounced 2 s after the listing lands.
    cargo build --quiet >>"$work/tests.log" 2>&1
    app="${CARGO_TARGET_DIR:-$root/target}/debug/cosmic-pass"
    COSMIC_PASS_CLI="$root/tests/fixtures/fake-pass-cli" \
        FAKE_FIXTURE_DIR="$root/tests/fixtures/pass-cli/synthetic" \
        COSMIC_SINGLE_INSTANCE=0 \
        timeout 6 "$app" --background >"$work/app.log" 2>&1 || true
    # No cache file means the ciphertext below was never scanned: a locked or unreachable
    # keyring keeps the app in memory-only mode (FR-024a), and so does a run cut short
    # before the debounce.
    if ! find "$COSMIC_PASS_CACHE_DIR" -type f -print -quit 2>/dev/null | grep -q .; then
        skipped+=("encrypted cache: no cache file was written (keyring locked, or the run ended before the 2 s debounce)")
    fi
fi

echo "==> live pass-cli suite"
if [ "${COSMIC_PASS_LIVE:-}" != 1 ]; then
    skipped+=("live pass-cli suite: set COSMIC_PASS_LIVE=1 and sign in to include it")
else
    # Read-only, and never scanned for markers: see the note at the top.
    COSMIC_PASS_LIVE=1 cargo nextest run --all-features --no-tests=pass \
        --run-ignored=only -E 'binary(pass_cli_live)' --no-capture \
        >"$work/live.log" 2>&1
    # Every line the suite prints itself must match one of these. Anything else is output
    # nobody vetted, which is where a secret would appear.
    allowed='^(SCENARIO |LATENCY |FIXTURE |vaults: |items: |plain items in the first vault: |totp fields: |fixtures compared: |pass-cli under test: |SKIP )'
    # nextest's own frames, cargo's progress, and blank lines.
    noise='^( *(PASS|FAIL|START|SKIP|Summary|Nextest|Starting|Compiling|Finished|running|test |test result:|---|────)|$)'
    if unvetted=$(grep -vE "$allowed" "$work/live.log" | grep -vE "$noise"); then
        echo "LEAK: the live suite printed lines outside its allowed vocabulary:" >&2
        printf '%s\n' "$unvetted" | head -20 >&2
        exit 1
    fi
    # The log is deleted rather than scanned: it holds no markers by construction, and keeping
    # it would put real account output in the directory the marker grep below prints.
    rm -f "$work/live.log"
fi

echo "==> scanning"
# -a, not -I: the cache is binary, and a cache written in the clear is exactly what this must
# catch, so binary files are searched instead of skipped.
if grep -ral -e 'SECRET-FIXTURE-' -e 'SECRETFIXTURE' "$work"; then
    echo "LEAK: fixture secrets found in the files above" >&2
    exit 1
fi

if [ ${#skipped[@]} -gt 0 ]; then
    printf 'SKIPPED: %s\n' "${skipped[@]}" >&2
    echo "INCONCLUSIVE: nothing leaked in what was scanned, but the steps above did not run" >&2
    exit 2
fi
echo "no leaks found"
