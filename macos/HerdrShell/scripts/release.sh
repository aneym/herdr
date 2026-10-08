#!/bin/bash
# release.sh <git-ref> [--install]
# Detached release worktree, prod bundle, stage under Application Support.
# --install moves the staged app to ~/Applications. Does not launch it.
set -euo pipefail
REF="${1:?git ref}"
INSTALL=0
if [[ "${2:-}" == "--install" ]]; then
  INSTALL=1
elif [[ -n "${2:-}" ]]; then
  echo "usage: release.sh <git-ref> [--install]" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SOURCE_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)"
COMMON="$(git -C "$SOURCE_ROOT" rev-parse --path-format=absolute --git-common-dir)"
MAIN="${HERDR_REPO:-$(cd "$(dirname "$COMMON")" && pwd)}"

worktrees() {
  git -C "$MAIN" worktree list --porcelain | while IFS= read -r line; do
    case "$line" in
      worktree\ *) printf '%s\n' "${line#worktree }" ;;
      /*) printf '%s\n' "${line%% *}" ;;
    esac
  done
}

WT="${HERDR_RELEASE_WORKTREE:-}"
if [[ -z "$WT" ]]; then
  while IFS= read -r wt; do
    if [[ "$(basename "$wt")" == "shell-release" ]]; then
      WT="$wt"
      break
    fi
  done < <(worktrees)
fi
if [[ -z "$WT" ]]; then
  WT="$(dirname "$SOURCE_ROOT")/shell-release"
fi

VENDOR_SRC="${HERDR_VENDOR:-}"
if [[ -z "$VENDOR_SRC" ]]; then
  while IFS= read -r wt; do
    [[ "$wt" == "$WT" ]] && continue
    cand="$wt/macos/HerdrShell/Vendor"
    if [[ -d "$cand" ]]; then
      VENDOR_SRC="$(cd "$cand" && pwd -P)"
      break
    fi
  done < <(worktrees)
fi
if [[ -z "$VENDOR_SRC" ]]; then
  echo "release.sh: no HerdrShell Vendor in the git worktree list" >&2
  exit 1
fi

COMMIT=$(git -C "$MAIN" rev-parse --verify "$REF^{commit}")
if [[ ! -e "$WT/.git" ]]; then
  git -C "$MAIN" worktree add --detach "$WT" "$COMMIT"
else
  git -C "$WT" checkout --detach "$COMMIT"
fi
if [[ ! -e "$WT/macos/HerdrShell/Vendor" ]]; then
  ln -s "$VENDOR_SRC" "$WT/macos/HerdrShell/Vendor"
fi
nice -n 10 swift build -c release --package-path "$WT/macos/HerdrShell"
"$WT/macos/HerdrShell/scripts/bundle.sh" prod "$WT/macos/HerdrShell/.build/bundle-prod"
STAGE="$HOME/Library/Application Support/HerdrShell/staged"
# Direct release.sh callers must obey the same no-downgrade contract as fanout.
INSTALLED=$(/usr/libexec/PlistBuddy -c 'Print :HerdrShellCommit' "$HOME/Applications/Herdr Shell.app/Contents/Info.plist" 2>/dev/null || true)
BUILT_APP="$WT/macos/HerdrShell/.build/bundle-prod/Herdr Shell.app"
BUILT_COMMIT=$(/usr/libexec/PlistBuddy -c 'Print :HerdrShellCommit' "$BUILT_APP/Contents/Info.plist")
if [[ ${#BUILT_COMMIT} -lt 12 || "$COMMIT" != "$BUILT_COMMIT"* ]]; then
  echo "release.sh: bundle commit mismatch: expected $COMMIT, got $BUILT_COMMIT" >&2
  exit 1
fi
if [[ -d "$HOME/Applications/Herdr Shell.app" ]]; then
  if ! git -C "$MAIN" rev-parse --verify "$INSTALLED^{commit}" >/dev/null 2>&1; then
    HERDR_SHELL_PUBLISHING=1 git -C "$MAIN" fetch -q origin || true
  fi
  if ! OLD=$(git -C "$MAIN" rev-parse --verify "$INSTALLED^{commit}" 2>/dev/null); then
    echo "release.sh: skip $COMMIT: installed commit unknown ($INSTALLED)"
    exit 1
  fi
  if [[ "$OLD" == "$COMMIT" ]]; then
    echo "release.sh: already installed $COMMIT"
    exit 0
  fi
  TWIN=""
  if ! git -C "$MAIN" merge-base --is-ancestor "$OLD" "$COMMIT"; then
    # An install whose commit was rebased or amended onto REF counts as the commit with its tree;
    # commits OLD already contains never match, so an older REF stays refused.
    TREE=$(git -C "$MAIN" rev-parse "$OLD^{tree}")
    SINCE=$(( $(git -C "$MAIN" log -1 --format=%ct "$OLD") - 86400 ))
    TWIN=$(git -C "$MAIN" log --since="$SINCE" --format='%H %T' "$COMMIT" "^$OLD" | awk -v t="$TREE" '$2 == t && !f { print $1; f = 1 }')
    if [[ -n "$TWIN" ]]; then
      echo "release.sh: installed $INSTALLED was rewritten as $TWIN (same tree)"
    fi
  fi
  if [[ "$TWIN" == "$COMMIT" ]]; then
    echo "release.sh: already installed $COMMIT"
    exit 0
  fi
  if ! git -C "$MAIN" merge-base --is-ancestor "${TWIN:-$OLD}" "$COMMIT"; then
    echo "release.sh: skip $COMMIT: not a strict descendant of installed $INSTALLED"
    exit 1
  fi
fi
mkdir -p "$STAGE"
INCOMING="$STAGE/Herdr Shell.app.incoming"
rm -rf "$INCOMING"
ditto "$BUILT_APP" "$INCOMING"
if [[ ! -x "$INCOMING/Contents/MacOS/HerdrShell" ]]; then
  echo "staged incoming failed verification" >&2
  exit 1
fi
PREV="$STAGE/Herdr Shell.app.previous"
rm -rf "$PREV"
if [[ -d "$STAGE/Herdr Shell.app" ]]; then
  mv "$STAGE/Herdr Shell.app" "$PREV"
fi
mv "$INCOMING" "$STAGE/Herdr Shell.app"
rm -rf "$PREV"
python3 - "$STAGE" "$REF" "$HOME/Applications/Herdr Shell.app" "$WT" <<'PY'
import json, os, subprocess, sys
stage, ref, installed, repo = sys.argv[1:]
plist = os.path.join(stage, "Herdr Shell.app", "Contents", "Info.plist")
commit = subprocess.check_output(
    ["/usr/libexec/PlistBuddy", "-c", "Print :HerdrShellCommit", plist], text=True
).strip()
built = subprocess.check_output(
    ["/usr/libexec/PlistBuddy", "-c", "Print :HerdrShellBuiltAt", plist], text=True
).strip()
prev = ""
ip = os.path.join(installed, "Contents", "Info.plist")
if os.path.isfile(ip):
    try:
        prev = subprocess.check_output(
            ["/usr/libexec/PlistBuddy", "-c", "Print :HerdrShellCommit", ip], text=True
        ).strip()
    except subprocess.CalledProcessError:
        prev = ""
notes = []
if prev and prev != commit:
    probe = subprocess.run(["git", "-C", repo, "cat-file", "-e", prev + "^{commit}"])
    if probe.returncode == 0:
        log = subprocess.run(
            ["git", "-C", repo, "log", "--format=%s", f"{prev}..{commit}"],
            capture_output=True, text=True,
        )
        notes = [ln for ln in log.stdout.splitlines() if ln][:8]
payload = {"commit": commit, "built_at": built, "ref": ref, "notes": notes}
with open(os.path.join(os.path.dirname(stage), "staged.json"), "w") as f:
    json.dump(payload, f, indent=2)
    f.write("\n")
print(json.dumps(payload))
PY
DEST="$HOME/Applications/Herdr Shell.app"
if [[ ! -d "$DEST" || "$INSTALL" == 1 ]]; then
  mkdir -p "$HOME/Applications"
  INCOMING="$HOME/Applications/Herdr Shell.app.incoming"
  rm -rf "$INCOMING"
  ditto "$STAGE/Herdr Shell.app" "$INCOMING"
  if [[ ! -x "$INCOMING/Contents/MacOS/HerdrShell" ]]; then
    echo "install incoming failed verification" >&2
    exit 1
  fi
  PREV="$HOME/Applications/Herdr Shell.app.previous"
  rm -rf "$PREV"
  if [[ -d "$DEST" ]]; then
    mv "$DEST" "$PREV"
  fi
  mv "$INCOMING" "$DEST"
  rm -rf "$PREV"
fi
echo "staged $STAGE"
if [[ -d "$DEST" ]]; then
  echo "installed $DEST"
fi
