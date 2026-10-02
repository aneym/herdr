#!/bin/bash
# Launch the app against the lab session only. The environment is rebuilt
# from lab.py's scrubbed env (no HERDR_ENV/HERDR_PANE_ID/CLAUDE* from the caller).
set -euo pipefail
D="$(cd "$(dirname "$0")/.." && pwd)"
LAB="$HOME/.cache/herdr-build/${SHELL_LAB:-shellspike}"
FIFO="$LAB/control.fifo"
BIN="${HERDR_SHELL_APP:-$D/.build/release/HerdrShell}"
envs=()
while IFS= read -r line; do envs+=("$line"); done < <(python3 "$D/scripts/lab.py" env)
SOCK=$(python3 "$D/scripts/lab.py" env | sed -n 's/^HERDR_SOCKET_PATH=//p')
# The app is interactive UI and runs at normal priority; the lab server is niced.
# Fixture paths for P15/P16. The app reads them before it clears HERDR_*.
for k in HERDR_LANES_PATH HERDR_AREAS_PATH SHELL_LAB CONTROL_MODES CONTROL_WORKFLOWS HERDR_KIND_BIN HERDR_LANE_BIN; do
  if [[ -n "${!k:-}" ]]; then envs+=("$k=${!k}"); fi
done
exec env -i "${envs[@]}" "$BIN" --herdr "$LAB/bin/herdr" --socket "$SOCK" --control "$FIFO" "$@" \
  >>"$LAB/app.log" 2>&1
