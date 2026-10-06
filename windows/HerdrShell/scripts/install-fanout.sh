#!/bin/bash
# Install the scheduler from a snapshot, never execute repository scripts in launchd.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
installed="$HOME/.local/share/herdr-winshell-fanout"
label='com.aneyman.herdr-winshell-fanout'
plist="$HOME/Library/LaunchAgents/$label.plist"
python3_path="$(command -v python3)"
mkdir -p "$installed/pc" "$HOME/Library/LaunchAgents"
# Stop the old job before replacing its script snapshot.
launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
cp "$here/fanout.py" "$here/pc.py" "$installed/"
cp "$here/"*.ps1 "$installed/"
cp "$here/pc/"*.ps1 "$installed/pc/"
"$python3_path" - "$here/$label.plist" "$plist" "$python3_path" "$installed/fanout.py" <<'PY'
import os
import plistlib
import sys
from pathlib import Path
source, destination, python, fanout = sys.argv[1:]
with open(source, 'rb') as handle:
    config = plistlib.load(handle)
config['ProgramArguments'] = [python, fanout]
temporary = Path(destination + '.tmp')
with temporary.open('wb') as handle:
    plistlib.dump(config, handle)
os.replace(temporary, destination)
PY
launchctl bootstrap "gui/$(id -u)" "$plist"
printf 'Installed %s\n' "$plist"
