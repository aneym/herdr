#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
lock="$HOME/.agent-rails/locks/studio-build"
mkdir -p "$(dirname "$lock")"
while ! mkdir "$lock" 2>/dev/null; do
    stamp=$(stat -f %m "$lock/owner" 2>/dev/null || stat -f %m "$lock")
    if (( $(date +%s) - stamp > 1800 )); then
        rm -f "$lock/owner"
        rmdir "$lock" 2>/dev/null || true
        continue
    fi
    echo 'waiting for the studio-build lock'
    sleep 20
done
trap 'rm -f "$lock/owner"; rmdir "$lock"' EXIT
printf 'desk-s2-build %s\n' "$(date +%s)" > "$lock/owner"
nice -n 10 swift build -c release
