#!/usr/bin/env sh
# Install (or reinstall) an OH Hive launchd agent on macOS. Idempotent.
#   scripts/service-mac.sh worker     # hive work  (compute node; needs Ollama running)
#   scripts/service-mac.sh server     # hive-server serve  (regional server; needs HIVE_PUBLIC_URL set)
#   scripts/service-mac.sh tunnel     # cloudflared tunnel run  (needs ~/.cloudflared/config.yml)
#   scripts/service-mac.sh hub        # hive hub serve   (this machine's vault, for its peers)
#   scripts/service-mac.sh bots       # hive bots work   (answer as this machine's agents)
#   scripts/service-mac.sh <name> stop
# Fetches the plist from the repo if this script is run standalone (curl | sh -s worker).
#
# A LAUNCH AGENT IS NOT A CONVENIENCE HERE, IT IS THE ONLY THING THAT WORKS. On macOS, access to
# other machines on the LAN is granted per responsible process. A binary started from a terminal
# inherits that terminal's grant, so `nohup ... &`, `screen`, and a detached subshell all appear
# to work -- right up until the login session that owns them ends, at which point the process is
# reparented to launchd, becomes its own responsible process, and every LAN connection fails with
# "No route to host" while loopback and the internet keep working perfectly.
#
# Measured on 2026-09-18, one detached probe every six seconds against a hub on the LAN:
#   21:12:45 OK ... 21:13:46 OK     <- ssh session alive
#   21:13:52 FAIL ... 21:14:53 FAIL <- same process, same binary, session closed
# Nothing else changed. A launchd agent is a first-class process with its own identity, which is
# what macOS can actually grant Local Network access to -- see the note printed after install.
set -eu
kind="${1:?worker|server|tunnel|hub|bots}"; action="${2:-start}"
case "$kind" in
  worker) label=media.happyjack.hive-worker; bin=hive ;;
  server) label=media.happyjack.hive-server; bin=hive-server ;;
  tunnel) label=media.happyjack.cloudflared-hive; bin=cloudflared ;;
  hub)    label=media.happyjack.hive-hub; bin=hive ;;
  bots)   label=media.happyjack.hive-bots; bin=hive ;;
  *) echo "worker|server|tunnel|hub|bots"; exit 1 ;;
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

# `hub` and `bots` are written here rather than templated from packaging/, because unlike the
# other three their arguments differ per machine -- which address this vault listens on, which
# hub this node answers through -- and `bots` genuinely has two shapes: on the hub machine
# itself it talks to its own vault directly and must NOT be given --hub at all.
write_args_plist() {
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' '<plist version="1.0">' '<dict>' \
    "  <key>Label</key><string>$label</string>" \
    '  <key>ProgramArguments</key>' '  <array>' > "$plist_dst"
  printf '    <string>%s</string>\n' "$HOME/.local/bin/$bin" >> "$plist_dst"
  for a in "$@"; do printf '    <string>%s</string>\n' "$a" >> "$plist_dst"; done
  printf '%s\n' '  </array>' \
    '  <key>EnvironmentVariables</key>' '  <dict><key>RUST_LOG</key><string>info</string></dict>' \
    '  <key>RunAtLoad</key><true/>' '  <key>KeepAlive</key><true/>' \
    '  <key>ThrottleInterval</key><integer>5</integer>' \
    "  <key>StandardOutPath</key><string>$HOME/Library/Logs/ohhive/$logname.log</string>" \
    "  <key>StandardErrorPath</key><string>$HOME/Library/Logs/ohhive/$logname.log</string>" \
    '</dict>' '</plist>' >> "$plist_dst"
}

case "$kind" in
  hub)
    logname=hive-hub
    # Must be this machine's WIRED LAN address, and the one carrying the default route. Binding
    # the other address of a dual-homed machine makes replies leave by a different interface than
    # requests arrived on, which peers see as connections that work intermittently for no visible
    # reason. `hive hub serve --help` has the long version.
    if [ -z "${HIVE_HUB_BIND:-}" ]; then
      iface=$(route -n get default 2>/dev/null | awk '/interface:/{print $2}')
      addr=$(ipconfig getifaddr "$iface" 2>/dev/null || echo "")
      echo "HIVE_HUB_BIND is required, e.g.:"
      [ -n "$addr" ] && echo "  HIVE_HUB_BIND=$addr:8787 $0 hub    # $iface, this machine's default route"
      [ -z "$addr" ] && echo "  HIVE_HUB_BIND=<wired-lan-ip>:8787 $0 hub"
      exit 1
    fi
    write_args_plist hub serve --bind "$HIVE_HUB_BIND"
    ;;
  bots)
    logname=hive-bots
    if [ -z "${HIVE_MODEL:-}" ]; then
      echo "HIVE_MODEL is required (the local model this node answers with), e.g.:"
      echo "  HIVE_MODEL=gemma4:12b-it-qat $0 bots"
      echo "  (add HIVE_BOTS_HUB=http://<hub-lan-ip>:8787 unless this machine IS the hub)"
      exit 1
    fi
    if [ -n "${HIVE_BOTS_HUB:-}" ]; then
      write_args_plist bots --hub "$HIVE_BOTS_HUB" work --model "$HIVE_MODEL" --poll "${HIVE_BOTS_POLL:-10}"
    else
      write_args_plist bots work --model "$HIVE_MODEL" --poll "${HIVE_BOTS_POLL:-10}"
    fi
    ;;
  *)
    src="$here/../packaging/$label.plist"
    if [ -f "$src" ]; then
      sed "s|__HOME__|$HOME|g" "$src" > "$plist_dst"
    else
      curl -fsSL "https://raw.githubusercontent.com/jackcanon/ohhive-releases/main/packaging/$label.plist" | sed "s|__HOME__|$HOME|g" > "$plist_dst"
    fi
    ;;
esac
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
  case "$kind" in
    hub|bots)
      echo "  If it reports \"No route to host\" reaching another machine, macOS has not granted"
      echo "  it Local Network access: System Settings → Privacy & Security → Local Network."
      echo "  Loopback and the internet keep working when that is the problem, which is what"
      echo "  makes it look like anything other than a permission."
      ;;
  esac
else
  echo "✗ $label did not start — check ~/Library/Logs/ohhive/"; exit 1
fi
