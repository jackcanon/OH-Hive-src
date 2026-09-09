#!/usr/bin/env bash
# Assembles a real "OH Hive.app" bundle from the SPM build -- no Xcode project needed. Run from
# anywhere; this script cd's to the package root itself.
#
# Local testing (ad-hoc signed, default):
#   scripts/build-app.sh
#
# Signed with your real Developer ID (same identity the Tauri dmg pipeline uses), for anything
# you intend to hand to someone else or notarize later:
#   OHHIVE_SIGN_IDENTITY="Developer ID Application: Jack Blair (5FLLB92M4A)" scripts/build-app.sh
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="OH Hive"
EXECUTABLE_NAME="OHHive"
BUNDLE_ID="media.happyjack.ohhive"
VERSION="0.3.0"
APP_DIR="$APP_NAME.app"
SIGN_IDENTITY="${OHHIVE_SIGN_IDENTITY:--}"
ICON_SRC="../desktop/src-tauri/icons/icon.icns"
# Same binary the Tauri app's build.rs fetches (ADR-013 D74/ADR-018 task #71) -- reused here
# rather than downloading a second copy. If it's missing, run the Tauri app's build once
# (cargo build in apps/desktop/src-tauri) to fetch it, or Tunnel setup will be unavailable here.
CLOUDFLARED_SRC="../desktop/src-tauri/resources/cloudflared-aarch64-apple-darwin"

echo "==> swift build -c release"
swift build -c release

echo "==> assembling $APP_DIR"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"
cp ".build/release/$EXECUTABLE_NAME" "$APP_DIR/Contents/MacOS/$EXECUTABLE_NAME"

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
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>$EXECUTABLE_NAME</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>LSMinimumSystemVersion</key><string>27.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST

echo "==> signing (identity: $SIGN_IDENTITY)"
codesign --force --deep --sign "$SIGN_IDENTITY" "$APP_DIR"

echo "==> done"
codesign -dv "$APP_DIR" 2>&1 | head -5
echo
echo "Launch with: open \"$APP_DIR\""
