#!/bin/bash
# Build and bundle the dev channel outside ~/Applications.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SOURCE_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"
registered=0
while IFS= read -r line; do
  case "$line" in
    worktree\ *) wt="${line#worktree }" ;;
    /*) wt="${line%% *}" ;;
    *) continue ;;
  esac
  if [[ "$wt" == "$SOURCE_ROOT" ]]; then
    registered=1
  fi
done < <(git -C "$SOURCE_ROOT" worktree list --porcelain)
if [[ "$registered" != 1 ]]; then
  echo "dev.sh: $SOURCE_ROOT is not in the git worktree list" >&2
  exit 1
fi
D="$SOURCE_ROOT/macos/HerdrShell"
cd "$D"
nice -n 10 swift build -c release
OUT="${HERDR_DEV_OUT:-$HOME/.cache/herdr-build/dev}"
mkdir -p "$OUT"
exec "$D/scripts/bundle.sh" dev "$OUT"
