#!/usr/bin/env bash
# Assembles a real Loki's Den app bundle from the SPM build -- no Xcode project needed. Run from
# anywhere; this script cd's to the package root itself.
#
# Local testing (ad-hoc signed, default):
#   scripts/build-app.sh
#
# Signed with your real Developer ID (same identity the Tauri dmg pipeline uses), for anything
# you intend to hand to someone else or notarize later:
#   OHHIVE_SIGN_IDENTITY="Developer ID Application: Jack Blair (5FLLB92M4A)" scripts/build-app.sh
#
# CI (release.yml) overrides the shipped version to match the release tag:
#   OHHIVE_APP_VERSION="0.3.0" scripts/build-app.sh
set -euo pipefail

cd "$(dirname "$0")/.."
PACKAGE_ROOT="$PWD"
# Single source of publisher client IDs; only public values enter Info.plist.
source "$PACKAGE_ROOT/config/oauth-clients.sh"
REPO_ROOT="$(cd ../.. && pwd)"
# Serialize generated binding updates and preserve a working bundle on build failures.
BUILD_LOCK="$PACKAGE_ROOT/.hive-app-build.lock"
if ! mkdir "$BUILD_LOCK" 2>/dev/null; then
    echo "Another Hive bundle build is active (or left $BUILD_LOCK behind)." >&2
    exit 1
fi
BUILD_STAGE=""
cleanup() {
    if [ -n "$BUILD_STAGE" ]; then
        if [ -e "$BUILD_STAGE/previous.app" ] && [ ! -e "$PACKAGE_ROOT/${APP_NAME:-Hive}.app" ]; then
            if ! mv "$BUILD_STAGE/previous.app" "$PACKAGE_ROOT/${APP_NAME:-Hive}.app"; then
                echo "Previous app preserved at $BUILD_STAGE/previous.app" >&2
                rmdir "$BUILD_LOCK"
                return
            fi
        fi
        rm -rf "$BUILD_STAGE"
    fi
    rmdir "$BUILD_LOCK"
}
trap cleanup EXIT
trap 'echo "Hive build failed; the previous app has been preserved." >&2' ERR
BUILD_STAGE="$(mktemp -d "$PACKAGE_ROOT/.hive-bundle.XXXXXX")"

# Three different names, deliberately, because they change at different costs.
#
# APP_DISPLAY_NAME is the brand: the Dock label, the menu bar, the About panel. Free to change.
#
# APP_BUNDLE_NAME is the .app directory on disk, which Finder shows. Jack made the call on
# 2026-09-18 to move it. The cost is real and worth stating: a member upgrading over an existing
# install ends up with BOTH apps in /Applications -- same bundle id, same data, two icons --
# until they delete Hive.app by hand. Nothing breaks, because the identifier below did not move,
# so settings and pairing carry over to whichever one they open. The release notes have to say
# this; the app cannot.
#
# BUNDLE_ID is not a name at all. It keys Application Support, the login item and the keychain
# entries, so changing it would orphan every existing install's data behind a directory nobody
# would think to look in. It does not move. Same reasoning for the `OHHive` Swift module and
# the `hive`/`hive-core` crate names -- internal identity, no brand value, real churn.
# Which source this bundle was actually built from. Twice in one evening the app was the odd
# one out -- once carrying a core twenty minutes older than the CLI fleet and contradicting it
# in the room, once carrying a teammate's uncommitted migrations and upgrading a live vault past
# what the committed code could open. Both took `strings` on a dylib to diagnose, because a
# bundle carries no record of where it came from. Now it does.
SOURCE_COMMIT="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
if [ -n "$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null)" ]; then
    SOURCE_COMMIT="$SOURCE_COMMIT-dirty"
fi
APP_DISPLAY_NAME="Loki's Den"
APP_BUNDLE_NAME="Loki's Den"
APP_NAME="$APP_BUNDLE_NAME"
EXECUTABLE_NAME="Hive"
BUNDLE_ID="media.happyjack.hive"
VERSION="${OHHIVE_APP_VERSION:-0.4.1}"
APP_DIR="$BUILD_STAGE/$APP_NAME.app"
SIGN_IDENTITY="${OHHIVE_SIGN_IDENTITY:--}"
# The approved Loki's Den mark (docs/lokis-den-brand-v1, brand guide approved by Jack
# 2026-09-17). The Tauri app's honeycomb icon is the Hive's, and the Hive is the community --
# not this workspace.
ICON_SRC="$REPO_ROOT/docs/lokis-den-brand-v1/icons/macos/Den.icns"
# Same binary the Tauri app's build.rs fetches (ADR-013 D74/ADR-018 task #71) -- reused here
# rather than downloading a second copy. If it's missing, run the Tauri app's build once
# (cargo build in apps/desktop/src-tauri) to fetch it, or Tunnel setup will be unavailable here.
CLOUDFLARED_SRC="../desktop/src-tauri/resources/cloudflared-aarch64-apple-darwin"

echo "==> building Rust and matching Swift bindings"
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --locked --release --target aarch64-apple-darwin -p hive-ffi --lib
FFI_DIR="$BUILD_STAGE/ffi"
mkdir -p "$FFI_DIR" "$BUILD_STAGE/bindings"
cp "$REPO_ROOT/target/aarch64-apple-darwin/release/libohhive_ffi.dylib" "$FFI_DIR/"
# Snapshot before generating/linking: subsequent Rust rebuilds cannot change this pair.
install_name_tool -id '@rpath/libohhive_ffi.dylib' "$FFI_DIR/libohhive_ffi.dylib"
cargo run --manifest-path "$REPO_ROOT/Cargo.toml" --locked --release --target aarch64-apple-darwin -p hive-ffi --bin uniffi-bindgen -- generate \
    --library "$FFI_DIR/libohhive_ffi.dylib" --language swift --out-dir "$BUILD_STAGE/bindings"
# Sources/OHHiveFFI/ holds ONLY generated output -- .gitignore line 39 excludes ohhive_ffi.swift,
# its single file -- so the directory is empty in git, and git does not track empty directories.
# It therefore does not exist in a clean checkout. On a developer machine it does, which is why
# this is invisible locally and fails on a fresh runner, after a nine-minute Rust build.
#
# Sources/ohhive_ffiFFI/ is NOT the same case and the mkdir below is belt-and-braces there: its
# module.modulemap IS tracked, so that directory does exist in a clean checkout. Worth stating,
# because assuming both were generated sends you chasing a second bug that is not there -- the
# Swift package genuinely requires that modulemap, and a "fix" that creates the directory without
# it fails later and less obviously, at `swift build`.
#
# This exact failure was diagnosed on 2026-09-16 and fixed with an inline mkdir in ci.yml (559bb2f)
# but not here, so release.yml -- which calls this script -- still had it and the v0.4.1 release
# build died on it. ci.yml's own comment predicted that: "kept in step with it on purpose -- if
# these two ever diverge, CI stops testing what we ship." They had diverged in precisely this way.
# Fixing it in the script fixes every caller: release.yml, a fresh contributor clone, and local runs.
mkdir -p Sources/OHHiveFFI Sources/ohhive_ffiFFI
cp "$BUILD_STAGE/bindings/ohhive_ffi.swift" Sources/OHHiveFFI/ohhive_ffi.swift
cp "$BUILD_STAGE/bindings/ohhive_ffiFFI.h" Sources/ohhive_ffiFFI/ohhive_ffiFFI.h

echo "==> building Swift app"
OHHIVE_FFI_LIBRARY_DIR="$FFI_DIR" swift build -c release \
    -Xlinker -rpath -Xlinker '@executable_path/../Frameworks'

echo "==> assembling $APP_NAME.app"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources" "$APP_DIR/Contents/Frameworks"
cp ".build/release/$EXECUTABLE_NAME" "$APP_DIR/Contents/MacOS/Hive-bin"
cp "$FFI_DIR/libohhive_ffi.dylib" "$APP_DIR/Contents/Frameworks/"
# Opt-in P2 diagnostic; not enabled in release builds until account/platform acceptance.
if [ "${HIVE_BUILD_COPILOT_CHECK:-0}" = "1" ]; then
    cargo build --manifest-path "$REPO_ROOT/crates/copilot-conformance/Cargo.toml" --locked \
        --features bundled-runtime --bin hive-copilot-check
    cp "$REPO_ROOT/crates/copilot-conformance/target/debug/hive-copilot-check" "$APP_DIR/Contents/MacOS/"
fi
xcrun swiftc scripts/Launcher.swift -O -o "$APP_DIR/Contents/MacOS/$EXECUTABLE_NAME"

if [ -f "$ICON_SRC" ]; then
    cp "$ICON_SRC" "$APP_DIR/Contents/Resources/AppIcon.icns"
else
    echo "    (no icon found at $ICON_SRC -- shipping without one)"
fi

if [ -f "$CLOUDFLARED_SRC" ]; then
    cp "$CLOUDFLARED_SRC" "$APP_DIR/Contents/Resources/cloudflared"
    chmod 755 "$APP_DIR/Contents/Resources/cloudflared"
else
    echo "    (no cloudflared binary found at $CLOUDFLARED_SRC -- Tunnel setup will be unavailable)"
fi

cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>OHHiveSourceCommit</key><string>$SOURCE_COMMIT</string>
    <key>CFBundleName</key><string>$APP_DISPLAY_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_DISPLAY_NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>$EXECUTABLE_NAME</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>LSMinimumSystemVersion</key><string>27.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <!-- Required for the Transcribe tab's on-device Apple Speech path (TranscribeEngine.swift,
         SpeechAnalyzer/SpeechTranscriber) -- without this key macOS refuses the speech-recognition
         TCC prompt outright rather than asking the user, even though we only feed it audio files
         rather than the live microphone. -->
    <key>NSSpeechRecognitionUsageDescription</key><string>Hive uses on-device speech recognition to transcribe audio files you choose, entirely on this Mac.</string>
</dict>
</plist>
PLIST

# Publisher client configuration. Google Desktop metadata is approved for distribution.
if [ -n "${HIVE_GITHUB_OAUTH_CLIENT_ID:-}" ]; then
    if [[ ! "$HIVE_GITHUB_OAUTH_CLIENT_ID" =~ ^[A-Za-z0-9_.-]+$ ]]; then
        echo "Invalid HIVE_GITHUB_OAUTH_CLIENT_ID" >&2
        exit 1
    fi
    /usr/bin/plutil -insert HiveGitHubOAuthClientID -string "$HIVE_GITHUB_OAUTH_CLIENT_ID" "$APP_DIR/Contents/Info.plist"
fi

if [ -n "${HIVE_GOOGLE_OAUTH_CLIENT_ID:-}" ]; then
    if [[ ! "$HIVE_GOOGLE_OAUTH_CLIENT_ID" =~ ^[A-Za-z0-9_-]+\.apps\.googleusercontent\.com$ ]]; then
        echo "Invalid HIVE_GOOGLE_OAUTH_CLIENT_ID" >&2
        exit 1
    fi
    plutil -insert HiveGoogleOAuthClientID -string "$HIVE_GOOGLE_OAUTH_CLIENT_ID" "$APP_DIR/Contents/Info.plist"
fi

# Keep the source credential outside git; only the distributed bundle contains its value.
if [ -n "${HIVE_GOOGLE_OAUTH_CREDENTIAL_JSON:-}" ]; then
    python3 "$PACKAGE_ROOT/scripts/embed-google-credential.py" \
        "$HIVE_GOOGLE_OAUTH_CREDENTIAL_JSON" "$APP_DIR/Contents/Info.plist" "$HIVE_GOOGLE_OAUTH_CLIENT_ID"
fi

echo "==> signing (identity: $SIGN_IDENTITY)"
SIGN_ARGS=(--force --sign "$SIGN_IDENTITY")
if [ "$SIGN_IDENTITY" != "-" ]; then
    SIGN_ARGS+=(--options runtime --timestamp)
fi
# Sign nested code before the outer bundle; do not rely on --deep to repair it.
codesign "${SIGN_ARGS[@]}" "$APP_DIR/Contents/Frameworks/libohhive_ffi.dylib"
if [ -f "$APP_DIR/Contents/MacOS/hive-copilot-check" ]; then
    codesign "${SIGN_ARGS[@]}" "$APP_DIR/Contents/MacOS/hive-copilot-check"
fi
codesign "${SIGN_ARGS[@]}" "$APP_DIR/Contents/MacOS/Hive-bin"
if [ -f "$APP_DIR/Contents/Resources/cloudflared" ]; then
    codesign "${SIGN_ARGS[@]}" "$APP_DIR/Contents/Resources/cloudflared"
fi
codesign "${SIGN_ARGS[@]}" "$APP_DIR"
codesign --verify --deep --strict "$APP_DIR"
./scripts/verify-app.sh "$APP_DIR"

# Publish only after verification. Keep a recoverable previous bundle through the rename.
PREVIOUS="$BUILD_STAGE/previous.app"
if [ -e "$APP_NAME.app" ]; then mv "$APP_NAME.app" "$PREVIOUS"; fi
if ! mv "$APP_DIR" "$APP_NAME.app"; then
    if [ -e "$PREVIOUS" ]; then mv "$PREVIOUS" "$APP_NAME.app"; fi
    exit 1
fi
case "$SOURCE_COMMIT" in
  *-dirty)
    echo "!! built from a DIRTY working tree ($SOURCE_COMMIT)." >&2
    echo "!! This bundle carries uncommitted changes -- including anyone else's. If it opens a" >&2
    echo "!! real vault it can migrate it past what committed code can read. Build from a clean" >&2
    echo "!! tree or a worktree at origin/main for anything that will touch live data." >&2
    ;;
esac
echo "==> done"
echo "Launch with: open \"$APP_NAME.app\""
