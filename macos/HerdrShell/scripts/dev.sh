#!/bin/bash
# Build and bundle the dev channel outside ~/Applications.
set -euo pipefail
D="$(cd "$(dirname "$0")/.." && pwd)"
cd "$D"
nice -n 10 swift build -c release
OUT="${HERDR_DEV_OUT:-$HOME/.cache/herdr-build/dev}"
mkdir -p "$OUT"
exec "$D/scripts/bundle.sh" dev "$OUT"
