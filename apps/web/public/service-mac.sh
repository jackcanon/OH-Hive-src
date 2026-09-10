#!/usr/bin/env sh
# Install (or reinstall) a Hive launchd agent on macOS. Idempotent.
#   scripts/service-mac.sh worker     # hive work  (compute node; needs Ollama running)
#   scripts/service-mac.sh server     # hive-server serve  (regional server; needs HIVE_PUBLIC_URL set)
#   scripts/service-mac.sh <name> stop
# Fetches the plist from the repo if this script is run standalone (curl | sh -s worker).
set -eu
kind="${1:?worker|server}"; action="${2:-start}"
case "$kind" in
  worker) label=media.happyjack.hive-worker; bin=hive ;;
  server) label=media.happyjack.hive-server; bin=hive-server ;;
  *) echo "worker|server"; exit 1 ;;
esac
plist_dst="$HOME/Library/LaunchAgents/$label.plist"
if [ "$action" = "stop" ]; then launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true; echo "→ $label stopped"; exit 0; fi

[ -x "$HOME/.local/bin/$bin" ] || { echo "$HOME/.local/bin/$bin missing — run: curl -fsSL https://ohghive.com/install.sh | sh"; exit 1; }
mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs/ohhive"
here=$(cd "$(dirname "$0")" 2>/dev/null && pwd || echo .)
src="$here/../packaging/$label.plist"
if [ -f "$src" ]; then
  sed "s|__HOME__|$HOME|g" "$src" > "$plist_dst"
else
  curl -fsSL "https://raw.githubusercontent.com/jackcanon/ohhive-releases/main/packaging/$label.plist" | sed "s|__HOME__|$HOME|g" > "$plist_dst"
fi
launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$plist_dst"
sleep 2
if launchctl print "gui/$(id -u)/$label" >/dev/null 2>&1; then
  echo "→ $label running; logs: ~/Library/Logs/ohhive/$( [ "$kind" = worker ] && echo hive-worker || echo hive-server ).log"
else
  echo "✗ $label did not start — check ~/Library/Logs/ohhive/"; exit 1
fi
