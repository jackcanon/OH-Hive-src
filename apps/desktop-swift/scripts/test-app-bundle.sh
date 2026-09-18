#!/usr/bin/env bash
# Exercise the real packaged launcher, including failures, without starting Hive workers.
set -euo pipefail
SCRIPTS="$(cd "$(dirname "$0")" && pwd)"
APP="${1:?Usage: test-app-bundle.sh /path/to/the .app bundle (quote it -- the name has a space and an apostrophe)}"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/hive-bundle-test.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT
COPY="$TEST_ROOT/Folder with spaces/$(basename "$APP")"
mkdir -p "$(dirname "$COPY")"
ditto "$APP" "$COPY"
codesign --verify --deep --strict "$COPY"
"$SCRIPTS/verify-app.sh" "$COPY"
ENGINE="$COPY/Contents/Frameworks/libohhive_ffi.dylib"
LAUNCHER="$COPY/Contents/MacOS/Hive"
mv "$ENGINE" "$ENGINE.saved"
if "$LAUNCHER" --hive-bundle-check > "$TEST_ROOT/missing.log" 2>&1; then
    echo 'FAIL: missing engine unexpectedly passed' >&2; exit 1
fi
grep -q "Hive couldn't start" "$TEST_ROOT/missing.log"
printf 'not a Mach-O library\n' > "$ENGINE"
if "$LAUNCHER" --hive-bundle-check > "$TEST_ROOT/corrupt.log" 2>&1; then
    echo 'FAIL: corrupt engine unexpectedly passed' >&2; exit 1
fi
grep -q "Hive couldn't start" "$TEST_ROOT/corrupt.log"
mv "$ENGINE.saved" "$ENGINE"
mv "$COPY/Contents/MacOS/Hive-bin" "$COPY/Contents/MacOS/Hive-bin.saved"
if "$LAUNCHER" --hive-bundle-check > "$TEST_ROOT/executable.log" 2>&1; then
    echo 'FAIL: missing app executable unexpectedly passed' >&2; exit 1
fi
grep -q 'app executable is missing' "$TEST_ROOT/executable.log"
echo 'PASS: relocated signed bundle, missing engine, corrupt engine, missing executable'
