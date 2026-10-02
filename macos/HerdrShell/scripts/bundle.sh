#!/bin/bash
# bundle.sh <prod|dev> <out-dir>
# Wraps .build/release/HerdrShell into an ad-hoc signed .app. Does not install it.
set -euo pipefail
CHANNEL="${1:?channel prod or dev}"
OUT="${2:?out dir}"
D="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$D/.build/release/HerdrShell"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN" >&2
  exit 1
fi
case "$CHANNEL" in
  prod)
    ID="com.aneyman.herdr-shell"
    NAME="Herdr Shell"
    ;;
  dev)
    ID="com.aneyman.herdr-shell.dev"
    NAME="Herdr Shell Dev"
    ;;
  *)
    echo "usage: bundle.sh prod|dev <out-dir>" >&2
    exit 2
    ;;
esac
ROOT="$(git -C "$D" rev-parse --show-toplevel)"
COMMIT="$(git -C "$ROOT" rev-parse --short=12 HEAD)"
BUILT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
APP="$OUT/$NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp -p "$BIN" "$APP/Contents/MacOS/HerdrShell"
if [[ -d "$D/Resources" ]]; then
  cp -R "$D/Resources/." "$APP/Contents/Resources/"
fi
ICON=""
if [[ -f "$APP/Contents/Resources/AppIcon.icns" ]]; then
  ICON="AppIcon"
fi
python3 - "$APP/Contents/Info.plist" "$ID" "$NAME" "$COMMIT" "$BUILT" "$ICON" <<'PY'
import plistlib, sys
path, bid, name, commit, built, icon = sys.argv[1:]
info = {
    "CFBundleIdentifier": bid,
    "CFBundleName": name,
    "CFBundleDisplayName": name,
    "CFBundleExecutable": "HerdrShell",
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": "1.0",
    "CFBundleVersion": "1",
    "CFBundleInfoDictionaryVersion": "6.0",
    "LSMinimumSystemVersion": "14.0",
    "NSHighResolutionCapable": True,
    "HerdrShellCommit": commit,
    "HerdrShellBuiltAt": built,
}
if icon:
    info["CFBundleIconFile"] = icon
with open(path, "wb") as f:
    plistlib.dump(info, f)
PY
codesign -s - --force --deep "$APP"
echo "$APP"
