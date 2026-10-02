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

if [[ ! -e "$WT/.git" ]]; then
  git -C "$MAIN" worktree add --detach "$WT" "$REF"
else
  git -C "$WT" checkout --detach "$REF"
fi
if [[ ! -e "$WT/macos/HerdrShell/Vendor" ]]; then
  ln -s "$VENDOR_SRC" "$WT/macos/HerdrShell/Vendor"
fi
nice -n 10 swift build -c release --package-path "$WT/macos/HerdrShell"
"$WT/macos/HerdrShell/scripts/bundle.sh" prod "$WT/macos/HerdrShell/.build/bundle-prod"
STAGE="$HOME/Library/Application Support/HerdrShell/staged"
mkdir -p "$STAGE"
rm -rf "$STAGE/Herdr Shell.app"
mv "$WT/macos/HerdrShell/.build/bundle-prod/Herdr Shell.app" "$STAGE/Herdr Shell.app"
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
  rm -rf "$DEST"
  mv "$STAGE/Herdr Shell.app" "$DEST"
fi
echo "staged $STAGE"
if [[ -d "$DEST" ]]; then
  echo "installed $DEST"
fi
