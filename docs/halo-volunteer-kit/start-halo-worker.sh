#!/bin/bash
# Project Halo volunteer worker -- macOS Apple Silicon.
# Run this from the folder you unzipped. It binds ONLY to your Tailscale address.
set -e
cd "$(dirname "$0")"
BIN=./ggml-rpc-server-macos-arm64

if ! command -v tailscale >/dev/null 2>&1 && [ ! -x /Applications/Tailscale.app/Contents/MacOS/Tailscale ]; then
  echo "Tailscale isn't installed. Install it from the App Store, sign in with the invite Jack sent, then run this again."; exit 1
fi
TS=$(command -v tailscale || echo /Applications/Tailscale.app/Contents/MacOS/Tailscale)
IP=$($TS ip -4 2>/dev/null | head -1)
if [ -z "$IP" ]; then echo "Tailscale is installed but not connected. Open it, sign in, and run this again."; exit 1; fi

# macOS Gatekeeper will otherwise refuse a downloaded binary.
xattr -d com.apple.quarantine "$BIN" 2>/dev/null || true
chmod +x "$BIN"

echo "Your Tailscale address: $IP   (send this to Jack)"
echo "Machine: $(sysctl -n machdep.cpu.brand_string)  RAM: $(( $(sysctl -n hw.memsize) / 1073741824 )) GB"
echo "Starting the worker. Leave this window open. Ctrl-C to stop; nothing stays on your machine."
echo "(It will print a scary warning about open networks -- that's why it's bound to Tailscale only.)"
echo
exec "$BIN" -H "$IP" -p 50052 -d MTL0
