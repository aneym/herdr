#!/bin/bash
# Capture Herdr Shell's stacks while it is Not Responding, before anyone relaunches it.
#
#   scripts/shell-hang-sample.sh            # one probe: keep a sample only if the app is hung
#   scripts/shell-hang-sample.sh --keep     # one probe: always keep the sample
#   scripts/shell-hang-sample.sh --watch    # probe every 60 s until stopped (no launchd)
#
# A probe is a 2 s /usr/bin/sample of the running app at 10 ms. The app counts as hung when
# its main thread spent under half the samples waiting for events in
# __CFRunLoopServiceMachPort. A hung probe is kept as
# ~/Library/Logs/herdr-shell-hang-<utc>.txt (symbolized main thread, every libghostty
# thread) and the script prints its path. SHELL_HANG_PID=<pid> probes another process.
# Read-only: it never signals, relaunches or types into the app. The 2026-10-07 hang:
# main thread in ghostty_surface_set_content_scale, a renderer thread in CVDisplayLink::stop().
set -euo pipefail

mode="${1:-}"
logs="$HOME/Library/Logs"

probe() {
  local pid tmp total idle out
  pid="${SHELL_HANG_PID:-$(pgrep -x HerdrShell | head -1 || true)}"
  if [[ -z "$pid" ]]; then echo "herdr-shell-hang: HerdrShell is not running"; return 0; fi
  tmp="$(mktemp -t herdr-shell-sample)"
  /usr/bin/sample "$pid" 2 10 -file "$tmp" >/dev/null 2>&1 || { rm -f "${tmp:?}"; echo "herdr-shell-hang: sample failed for $pid"; return 1; }
  # Main thread block: from its header to the next thread header.
  read -r total idle < <(awk '
    /^    [0-9]+ Thread_/ { inmain = ($0 ~ /com\.apple\.main-thread/); if (inmain) total = $1; next }
    inmain && /__CFRunLoopServiceMachPort/ { for (i = 1; i <= NF; i++) if ($i ~ /^[0-9]+$/) { idle += $i; break } }
    END { print total + 0, idle + 0 }' "$tmp")
  if [[ "$total" -gt 0 && $((idle * 2)) -lt "$total" ]] || [[ "$mode" == "--keep" ]]; then
    out="$logs/herdr-shell-hang-$(date -u +%Y%m%dT%H%M%SZ).txt"
    { echo "pid $pid main-thread samples $total idle $idle"; ps -o pid,rss,vsz,%cpu,etime -p "$pid"; cat "$tmp"; } > "$out"
    echo "herdr-shell-hang: pid $pid main thread idle $idle/$total, saved $out"
  else
    echo "herdr-shell-hang: pid $pid answering (main thread idle $idle/$total)"
  fi
  rm -f "${tmp:?}"
}

if [[ "$mode" == "--watch" ]]; then
  while true; do probe || true; sleep 60; done
else
  probe
fi
