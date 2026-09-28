#!/bin/bash
# Copy the prebuilt static libghostty and its header into this package.
# Ghostty is pinned to b1d2b7e (1.3.2-dev). Set GHOSTTY_SRC to a checkout at that commit
# (default: the spikes checkout). Build it first (zig 0.16), under nice:
#   nice -n 10 zig build -Demit-xcframework=true -Dxcframework-target=native
set -euo pipefail
D="$(cd "$(dirname "$0")/.." && pwd)"
PIN=b1d2b7e
SRC="${GHOSTTY_SRC:-/Volumes/StudioExt/repos/herdr-shell-spikes/vendor/ghostty}"
HEAD="$(git -C "$SRC" rev-parse --short=7 HEAD)"
[ "$HEAD" = "$PIN" ] || { echo "Ghostty at $SRC is $HEAD, expected $PIN" >&2; exit 1; }
XC="$SRC/macos/GhosttyKit.xcframework/macos-arm64"
mkdir -p "$D/Vendor/GhosttyKit" "$D/Sources/GhosttyKit/include"
cp "$XC/libghostty-internal.a" "$D/Vendor/GhosttyKit/"
cp "$XC/Headers/ghostty.h" "$D/Sources/GhosttyKit/include/"
echo "vendored $(du -h "$D/Vendor/GhosttyKit/libghostty-internal.a" | cut -f1) from $XC"
