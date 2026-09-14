#!/usr/bin/env bash
# Assembles HaloBench.app from the SPM build -- no Xcode project needed. Same shape as
# apps/desktop-swift/scripts/build-app.sh. Run from anywhere.
#
#   scripts/build-app.sh                       # ad-hoc signed, for the lab
#   OHHIVE_SIGN_IDENTITY="Developer ID Application: Jack Blair (5FLLB92M4A)" scripts/build-app.sh
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="HaloBench"
BUNDLE_ID="media.happyjack.halobench"
VERSION="0.1.0"
APP_DIR="$APP_NAME.app"
# Sign with the Developer ID when it's in the keychain. Ad-hoc signing changes the app's code
# identity on every rebuild, so macOS forgets its Files-and-Folders / Local Network grants each
# time (Jack had to re-approve the JBOD volume on every launch). A real identity is stable.
DEFAULT_IDENTITY="Developer ID Application: Jack Blair (5FLLB92M4A)"
if [ -z "${OHHIVE_SIGN_IDENTITY:-}" ] && security find-identity -v -p codesigning 2>/dev/null | grep -q "$DEFAULT_IDENTITY"; then
    SIGN_IDENTITY="$DEFAULT_IDENTITY"
else
    SIGN_IDENTITY="${OHHIVE_SIGN_IDENTITY:--}"
fi

echo "==> swift build -c release"
swift build -c release

echo "==> icon"
rm -rf "$APP_NAME.iconset" "$APP_NAME.icns"
swift scripts/make-icon.swift "$APP_NAME.iconset"
iconutil -c icns "$APP_NAME.iconset"
rm -rf "$APP_NAME.iconset"

echo "==> assembling $APP_DIR"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp ".build/release/$APP_NAME" "$APP_DIR/Contents/MacOS/$APP_NAME"
cp "$APP_NAME.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"

cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>$APP_NAME</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>LSMinimumSystemVersion</key><string>27.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>NSLocalNetworkUsageDescription</key><string>HaloBench connects to the lab's RPC workers over the wired LAN to run split benchmarks.</string>
</dict>
</plist>
PLIST

echo "==> signing (identity: $SIGN_IDENTITY)"
if [ "$SIGN_IDENTITY" = "-" ]; then
    codesign --force --deep --sign - "$APP_DIR"
else
    codesign --force --deep --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP_DIR"
fi
codesign -dv "$APP_DIR" 2>&1 | grep -E "Signature|TeamIdentifier"
echo "==> done -- open \"$APP_DIR\""
