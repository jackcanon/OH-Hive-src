#!/usr/bin/env sh
# Install (or reinstall) an OH Hive launchd agent on macOS. Idempotent.
#   scripts/service-mac.sh worker     # hive work  (compute node; needs Ollama running)
#   scripts/service-mac.sh server     # hive-server serve  (regional server; needs HIVE_PUBLIC_URL set)
#   scripts/service-mac.sh tunnel     # cloudflared tunnel run  (needs ~/.cloudflared/config.yml)
#   scripts/service-mac.sh <name> stop
# Fetches the plist from the repo if this script is run standalone (curl | sh -s worker).
set -eu
kind="${1:?worker|server|tunnel}"; action="${2:-start}"
case "$kind" in
  worker) label=media.happyjack.hive-worker; bin=hive ;;
  server) label=media.happyjack.hive-server; bin=hive-server ;;
  tunnel) label=media.happyjack.cloudflared-hive; bin=cloudflared ;;
  *) echo "worker|server|tunnel"; exit 1 ;;
esac
plist_dst="$HOME/Library/LaunchAgents/$label.plist"
if [ "$action" = "stop" ]; then launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true; echo "→ $label stopped"; exit 0; fi

if [ ! -x "$HOME/.local/bin/$bin" ]; then
  if [ "$kind" = "tunnel" ]; then
    echo "$HOME/.local/bin/$bin missing — see docs/JOIN.md 'Make it reachable' to install cloudflared"
  else
    echo "$HOME/.local/bin/$bin missing — run: curl -fsSL https://ohghive.com/install.sh | sh"
  fi
  exit 1
fi
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
  case "$kind" in
    worker) logname=hive-worker ;;
    server) logname=hive-server ;;
    tunnel) logname=cloudflared-hive ;;
  esac
  echo "→ $label running; logs: ~/Library/Logs/ohhive/$logname.log"
else
  echo "✗ $label did not start — check ~/Library/Logs/ohhive/"; exit 1
fi
