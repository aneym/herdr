#!/bin/bash
# install-publish.sh: set up one-click Herdr Shell updates on Studio. Rerun after
# publish.py changes. Installs:
#   ~/.local/bin/herdr-shell-publish                 a copy of publish.py (launchd can't read /Volumes)
#   <herdr repo>/.git/hooks/reference-transaction    a push to origin/<release branch> runs `herdr-shell-publish auto`
#   ~/Library/LaunchAgents/com.aneyman.herdr-shell-fanout.plist   `herdr-shell-publish fanout` every 5 min
#   ~/Library/LaunchAgents/com.aneyman.herdr-shell-data.plist     `herdr-shell-publish data` every 20 s
#   ~/.config/herdr-shell/targets.json               only if missing (Book over ssh)
set -euo pipefail
D="$(cd "$(dirname "$0")" && pwd)"
REPO="${HERDR_REPO:-/Volumes/StudioExt/repos/herdr}"
HOOKS="$(git -C "$REPO" rev-parse --path-format=absolute --git-common-dir)/hooks"
BIN="$HOME/.local/bin/herdr-shell-publish"
LABEL=com.aneyman.herdr-shell-fanout
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"

install -m 755 "$D/publish.py" "$BIN"
install -m 755 "$D/../bin/herdr-shell-remote" "$HOME/.local/bin/herdr-shell-remote"

git -C "$REPO" config herdr-shell.releaseBranch >/dev/null || git -C "$REPO" config herdr-shell.releaseBranch feat/native-shell-latest

HOOK="$HOOKS/reference-transaction"
if [[ -e "$HOOK" ]] && ! grep -q herdr-shell-publish "$HOOK"; then
  echo "install-publish: $HOOK exists and isn't ours; add the herdr-shell-publish block by hand" >&2
  exit 1
fi
cat >"$HOOK" <<'SH'
#!/bin/bash
# herdr-shell-publish: when origin/<release branch> moves (push or fetch), build and stage
# Herdr Shell in the background. Runs on every ref update in this repo, so it stays cheap.
[[ "${1:-}" == committed && -z "${HERDR_SHELL_PUBLISHING:-}" ]] || exit 0
want="refs/remotes/origin/$(git config herdr-shell.releaseBranch || echo feat/native-shell-latest)"
hit=0
while read -r _old new ref; do
  [[ "$ref" == "$want" && "$new" != 0000000000000000000000000000000000000000 ]] && hit=1
done
if [[ "$hit" == 1 && -x "$HOME/.local/bin/herdr-shell-publish" ]]; then
  mkdir -p "$HOME/.cache/herdr-shell-publish"
  HERDR_SHELL_PUBLISHING=1 nohup "$HOME/.local/bin/herdr-shell-publish" auto \
    >>"$HOME/.cache/herdr-shell-publish/auto.log" 2>&1 </dev/null &
fi
exit 0
SH
chmod 755 "$HOOK"

mkdir -p "$HOME/.config/herdr-shell"
if [[ ! -f "$HOME/.config/herdr-shell/targets.json" ]]; then
  cat >"$HOME/.config/herdr-shell/targets.json" <<'JSON'
{"targets": [{"name": "book", "ssh": ["ssh", "macbook-ts"]}]}
JSON
fi

mkdir -p "$HOME/.cache/herdr-shell-publish"
cat >"$PLIST" <<XML
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$BIN</string><string>fanout</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>PATH</key><string>$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string></dict>
  <key>RunAtLoad</key><true/>
  <key>StartInterval</key><integer>300</integer>
  <key>StandardOutPath</key><string>$HOME/.cache/herdr-shell-publish/fanout.launchd.log</string>
  <key>StandardErrorPath</key><string>$HOME/.cache/herdr-shell-publish/fanout.launchd.log</string>
</dict>
</plist>
XML
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"

# Areas/Parked inputs for the other machines' apps, every 20 s when they change.
DLABEL=com.aneyman.herdr-shell-data
DPLIST="$HOME/Library/LaunchAgents/$DLABEL.plist"
sed -e "s/$LABEL/$DLABEL/" -e "s|<string>fanout</string>|<string>data</string>|" \
    -e "s|<integer>300</integer>|<integer>20</integer>|" -e "s|fanout.launchd.log|data.launchd.log|g" "$PLIST" >"$DPLIST"
launchctl bootout "gui/$(id -u)/$DLABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$DPLIST"
echo "installed $BIN, $HOOK, $PLIST, $DPLIST"
