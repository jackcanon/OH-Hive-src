#!/usr/bin/env bash
# Structural and loader checks, without starting workers or opening the app UI.
set -euo pipefail
APP_DIR="${1:?Usage: verify-app.sh /path/to/Hive.app}"
ENGINE="$APP_DIR/Contents/Frameworks/libohhive_ffi.dylib"
APP_BIN="$APP_DIR/Contents/MacOS/Hive-bin"
test -f "$ENGINE"
test -x "$APP_BIN"
# Reject checkout/build-machine or Homebrew dependencies in both shipped Mach-O files.
for binary in "$APP_BIN" "$ENGINE" "$APP_DIR/Contents/MacOS/Hive"; do
    while IFS= read -r dependency; do
        case "$dependency" in
            @rpath/*|@executable_path/*|@loader_path/*|/System/Library/*|/usr/lib/*) ;;
            *) echo "Nonportable library dependency: $dependency" >&2; exit 1 ;;
        esac
    done < <(otool -L "$binary" | tail -n +2 | sed -E 's/^[[:space:]]+//; s/ \(compatibility version.*$//')
done
otool -L "$APP_BIN" | grep -q '@rpath/libohhive_ffi.dylib'
"$APP_DIR/Contents/MacOS/Hive" --hive-bundle-check
