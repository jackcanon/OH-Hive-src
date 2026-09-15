#!/bin/bash
# Builds NativePilotApp and assembles + ad-hoc signs a real .app bundle around it.
# Run this from anywhere; it cd's to its own directory first.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

echo "== swift build -c release =="
swift build -c release

APP="NativePilotApp.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"

cp .build/release/NativePilotApp "$APP/Contents/MacOS/NativePilotApp"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>NativePilotApp</string>
    <key>CFBundleIdentifier</key>
    <string>com.ohhive.NativePilotApp</string>
    <key>CFBundleName</key>
    <string>NativePilotApp</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
PLIST

echo "== codesign (ad-hoc) =="
codesign --force --deep --sign - --timestamp=none "$APP"

echo
echo "== codesign -dv --verbose=4 (identity check) =="
codesign -dv --verbose=4 "$APP" 2>&1

echo
echo "Built: $(pwd)/$APP"
echo "Next: open \"$APP\"   (NOT running the binary inside it directly -- launching via"
echo "'open' is what gives this a real LaunchServices-registered identity to test)."
