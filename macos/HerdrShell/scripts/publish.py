#!/usr/bin/env python3
"""herdr-shell-publish: build Herdr Shell on Studio and stage it on every machine.

  herdr-shell-publish [REF]     build REF (default: origin/<release branch>) with
                                release.sh (stage only), keep it as the release,
                                then fan it out
  herdr-shell-publish auto      publish only if origin/<release branch> moved past
                                the release; called by the repo's reference-transaction
                                hook, so a push publishes by itself
  herdr-shell-publish fanout    copy the release into each target's staged dir
                                (launchd every 5 min; never reads the repo)
  herdr-shell-publish status    release commit and each target's installed/staged commit

On each machine the running app watches ~/Library/Application Support/HerdrShell.
When staged.json names a commit it isn't running, the title bar shows "Update";
one click swaps ~/Applications/Herdr Shell.app and relaunches. Nothing here
installs over or launches an app.

Release branch: `git config herdr-shell.releaseBranch` in the herdr repo
(default feat/native-shell-latest). Targets: ~/.config/herdr-shell/targets.json,
  {"targets": [{"name": "book", "ssh": ["ssh", "macbook-ts"]}]}
Studio itself is staged by release.sh. An unreachable target is skipped quietly and
picked up on the next fanout. Install: macos/HerdrShell/scripts/install-publish.sh.
"""
import fcntl
import json
import os
import shlex
import shutil
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
REPO = os.environ.get("HERDR_REPO", "/Volumes/StudioExt/repos/herdr")
APP = "Herdr Shell.app"
STORE = os.environ.get("HERDR_SHELL_RELEASE_DIR", f"{HOME}/.local/share/herdr-shell/release")
TARGETS = os.environ.get("HERDR_SHELL_TARGETS", f"{HOME}/.config/herdr-shell/targets.json")
LOGDIR = f"{HOME}/.cache/herdr-shell-publish"
LOCK = f"{LOGDIR}/publish.lock"
STAGE = f"{HOME}/Library/Application Support/HerdrShell"
SSH_OPTS = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=8"]
# Run from a git hook, the environment carries GIT_DIR and friends for the pushing worktree.
for _k in [k for k in os.environ if k.startswith("GIT_")]:
    del os.environ[_k]


def log(msg):
    os.makedirs(LOGDIR, exist_ok=True)
    line = f"[{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}] {msg}"
    with open(f"{LOGDIR}/publish.log", "a") as f:
        f.write(line + "\n")
    print(line, flush=True)


def git(*a):
    return subprocess.run(["git", "-C", REPO, *a], capture_output=True, text=True, check=True).stdout.strip()


def branch():
    r = subprocess.run(["git", "-C", REPO, "config", "herdr-shell.releaseBranch"], capture_output=True, text=True)
    return r.stdout.strip() or "feat/native-shell-latest"


def release_worktree():
    """The detached `shell-release` worktree release.sh builds in; created if missing."""
    for line in git("worktree", "list", "--porcelain").splitlines():
        if line.startswith("worktree ") and os.path.basename(line[9:]) == "shell-release":
            return line[9:]
    wt = os.path.join(os.path.dirname(REPO), "herdr-worktrees", "shell-release")
    git("worktree", "add", "--detach", wt, "HEAD")
    return wt


def release():
    try:
        with open(f"{STORE}/release.json") as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def targets():
    try:
        with open(TARGETS) as f:
            return json.load(f).get("targets", [])
    except (OSError, ValueError):
        return []


def same(a, b):
    return bool(a) and bool(b) and (a.startswith(b) or b.startswith(a))


def publish(ref):
    sha = git("rev-parse", "--verify", ref + "^{commit}")
    log(f"build {ref} = {sha[:12]}")
    # release.sh comes from the commit being built: check it out in the release worktree first.
    wt = release_worktree()
    subprocess.run(["git", "-C", wt, "checkout", "-q", "--detach", sha], check=True)
    script = f"{wt}/macos/HerdrShell/scripts/release.sh"
    with open(f"{LOGDIR}/build.log", "a") as out:
        r = subprocess.run(["bash", script, sha], stdout=out, stderr=subprocess.STDOUT,
                           env={**os.environ, "HERDR_REPO": REPO})
    if r.returncode != 0:
        log(f"FAIL release.sh {sha[:12]} exit {r.returncode} (see {LOGDIR}/build.log)")
        return False
    with open(f"{STAGE}/staged.json") as f:
        meta = json.load(f)
    if not same(meta.get("commit", ""), sha):
        log(f"FAIL staged.json names {meta.get('commit')} after building {sha[:12]}")
        return False
    # Keep our own copy: Studio's staged app is consumed when Studio clicks Update.
    os.makedirs(os.path.dirname(STORE), exist_ok=True)
    tmp = STORE + ".incoming"
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    subprocess.run(["ditto", f"{STAGE}/staged/{APP}", f"{tmp}/{APP}"], check=True)
    with open(f"{tmp}/release.json", "w") as f:
        json.dump(meta, f, indent=2)
        f.write("\n")
    old = STORE + ".previous"
    shutil.rmtree(old, ignore_errors=True)
    if os.path.isdir(STORE):
        os.rename(STORE, old)
    os.rename(tmp, STORE)
    shutil.rmtree(old, ignore_errors=True)
    log(f"release {meta['commit']} staged on studio")
    fanout()
    return True


REMOTE_PROBE = r'''
S="$HOME/Library/Application Support/HerdrShell"
i=$(/usr/libexec/PlistBuddy -c "Print :HerdrShellCommit" "$HOME/Applications/Herdr Shell.app/Contents/Info.plist" 2>/dev/null)
s=$(/usr/bin/plutil -extract commit raw -o - "$S/staged.json" 2>/dev/null)
a=$(/usr/libexec/PlistBuddy -c "Print :HerdrShellCommit" "$S/staged/Herdr Shell.app/Contents/Info.plist" 2>/dev/null)
echo "installed=$i"; echo "staged=$s"; echo "staged_app=$a"
'''

# The app is written first and checked; staged.json last, with a rename, so a
# running app never sees a commit whose bundle isn't complete.
REMOTE_DELIVER = r'''
set -e
S="$HOME/Library/Application Support/HerdrShell"
mkdir -p "$S/staged"
IN="$S/staged/.incoming"
rm -rf "$IN"; mkdir "$IN"
tar -xzf - -C "$IN"
/usr/bin/codesign --verify "$IN/Herdr Shell.app"
rm -rf "$S/staged/Herdr Shell.app"
mv "$IN/Herdr Shell.app" "$S/staged/Herdr Shell.app"
mv "$IN/release.json" "$S/staged.json.tmp"
rmdir "$IN"
mv "$S/staged.json.tmp" "$S/staged.json"
echo delivered
'''


def probe(t):
    r = subprocess.run(t["ssh"][:1] + SSH_OPTS + t["ssh"][1:] + ["/bin/sh", "-c", shlex.quote(REMOTE_PROBE)],
                       capture_output=True, text=True, timeout=60)
    if r.returncode != 0:
        return None
    return dict(l.split("=", 1) for l in r.stdout.splitlines() if "=" in l)


def fanout():
    rel = release()
    if not rel:
        log("fanout: no release yet")
        return
    commit = rel["commit"]
    for t in targets():
        name = t.get("name", "?")
        try:
            st = probe(t)
        except subprocess.TimeoutExpired:
            st = None
        if st is None:
            log(f"{name}: unreachable, will retry")
            continue
        if same(st.get("installed", ""), commit):
            continue
        if same(st.get("staged", ""), commit) and same(st.get("staged_app", ""), commit):
            continue
        tar = subprocess.Popen(["tar", "-czf", "-", "-C", STORE, APP, "release.json"], stdout=subprocess.PIPE)
        r = subprocess.run(t["ssh"][:1] + SSH_OPTS + t["ssh"][1:] + ["/bin/sh", "-c", shlex.quote(REMOTE_DELIVER)],
                           stdin=tar.stdout, capture_output=True, text=True, timeout=900)
        tar.stdout.close()
        tar.wait()
        if r.returncode == 0 and "delivered" in r.stdout:
            log(f"{name}: staged {commit} (installed {st.get('installed') or 'none'})")
        else:
            log(f"{name}: FAIL deliver {commit}: {(r.stderr or r.stdout).strip()[-300:]}")


def auto():
    os.makedirs(LOGDIR, exist_ok=True)
    with open(LOCK, "w") as lk:
        try:
            fcntl.flock(lk, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            # A publish is running; it re-reads the branch when it finishes.
            return
        tried = set()
        while True:
            ref = f"refs/remotes/origin/{branch()}"
            try:
                sha = git("rev-parse", "--verify", ref)
            except subprocess.CalledProcessError:
                log(f"auto: no {ref}")
                return
            rel = release()
            if (rel and same(rel.get("commit", ""), sha)) or sha in tried:
                return
            tried.add(sha)
            publish(sha)


def status():
    rel = release()
    print(json.dumps({"branch": branch(), "release": rel and rel.get("commit")}))
    for t in targets():
        print(json.dumps({"target": t.get("name"), **(probe(t) or {"unreachable": True})}))


def main():
    a = sys.argv[1:]
    if a and a[0] in ("-h", "--help"):
        print(__doc__)
    elif a and a[0] == "auto":
        auto()
    elif a and a[0] == "fanout":
        fanout()
    elif a and a[0] == "status":
        status()
    else:
        ref = a[0] if a else f"origin/{branch()}"
        os.makedirs(LOGDIR, exist_ok=True)
        with open(LOCK, "w") as lk:
            fcntl.flock(lk, fcntl.LOCK_EX)
            sys.exit(0 if publish(ref) else 1)


if __name__ == "__main__":
    main()
