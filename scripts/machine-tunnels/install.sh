#!/bin/bash
# install.sh [--via <ssh-host>]
# Installs herdr-machine-tunnels and its launchd agent on this Mac, and writes
# ~/.config/herdr-machines/tunnels.json unless one exists. Without --via the
# machines are reached directly (Studio); with --via they are reached through
# that host's own forwarded sockets (Book: --via studio).
set -euo pipefail
D="$(cd "$(dirname "$0")" && pwd)"
VIA=""
if [[ "${1:-}" == "--via" ]]; then VIA="${2:?host}"; fi
mkdir -p "$HOME/.local/bin" "$HOME/.config/herdr-machines" "$HOME/Library/Logs"
install -m 755 "$D/herdr-machine-tunnels" "$HOME/.local/bin/herdr-machine-tunnels"
CFG="$HOME/.config/herdr-machines/tunnels.json"
if [[ ! -e "$CFG" ]]; then
  python3 - "$CFG" "$VIA" <<'PY'
import json, sys
cfg, via = sys.argv[1], sys.argv[2]
# name, ssh alias from Studio, remote herdr dir
machines = [("ax42", "ax42", "/home/jobs/.config/herdr"),
            ("pc", "pc-wsl", "/home/jobs/.config/herdr"),
            ("forge", "forge-lanes", "/home/jobs/.config/herdr")]
out = []
for name, alias, rdir in machines:
    if via:
        out.append({"name": name, "ssh": ["ssh", via], "remote_dir": f"/Users/aneyman/.config/herdr-machines/{name}"})
    else:
        out.append({"name": name, "ssh": ["ssh", alias], "remote_dir": rdir})
json.dump({"machines": out}, open(cfg, "w"), indent=1)
PY
fi
PLIST="$HOME/Library/LaunchAgents/com.aneyman.herdr-machine-tunnels.plist"
cat > "$PLIST" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.aneyman.herdr-machine-tunnels</string>
  <key>ProgramArguments</key><array>
    <string>/usr/bin/python3</string><string>$HOME/.local/bin/herdr-machine-tunnels</string><string>run</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/herdr-machine-tunnels.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/herdr-machine-tunnels.log</string>
</dict></plist>
PL
launchctl bootout "gui/$(id -u)/com.aneyman.herdr-machine-tunnels" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "installed; config $CFG"
