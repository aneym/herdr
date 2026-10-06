#!/usr/bin/env python3
"""herdr-shell-publish: build Herdr Shell on Studio and stage it on every machine.

  herdr-shell-publish [REF]     build REF (default: origin/<release branch>) with
                                release.sh (stage only), keep it as the release,
                                then fan it out
  herdr-shell-publish auto      publish only if origin/<release branch> moved past
                                the release and changed macos/HerdrShell; called by the
                                repo's reference-transaction hook, so a push publishes
                                by itself
  herdr-shell-publish watch     fetch origin/<release branch>, then `auto`; launchd every 30 s,
                                so a push from any machine publishes within a minute
  herdr-shell-publish fanout    copy the release into each target's staged dir
                                (launchd every 5 min; never reads the repo)
  herdr-shell-publish data      copy Studio's lanes/areas/modes files to each target's
                                ~/.agent-rails/herdr when they change (launchd every 20 s),
                                so Areas and Parked draw there; read-only copies
  herdr-shell-publish install NAME
                                put the release in NAME's ~/Applications now (a new
                                machine, or one far behind); refuses while it runs there
  herdr-shell-publish status    release commit and each target's installed/staged commit

On each machine the running app watches ~/Library/Application Support/HerdrShell.
When staged.json names a commit it isn't running, the title bar shows "Update";
one click swaps ~/Applications/Herdr Shell.app and relaunches. Nothing here
installs over or launches an app.

Release branch: `git config herdr-shell.releaseBranch` in the herdr repo
(default main). Targets: ~/.config/herdr-shell/targets.json,
  {"targets": [{"name": "book", "ssh": ["ssh", "macbook-ts"],
                "server": {"ssh": ["ssh", "studio-ts"], "remote_bin": "~/.local/bin/herdr-shell-remote"}}]}
A target's "server" becomes its ~/.config/herdr-shell/server.json when it has none.
A target with "local": true (Studio, {"name": "studio", "local": true}) runs the same
scripts here without ssh; release.sh stages Studio at build time and fanout restages it
if that staging was lost. An unreachable target is skipped quietly and
picked up on the next fanout. Install: macos/HerdrShell/scripts/install-publish.sh.
"""
from contextlib import contextmanager
from functools import wraps
import fcntl
import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys
import time
import tempfile

HOME = os.path.expanduser("~")
REPO = os.environ.get("HERDR_REPO", "/Volumes/StudioExt/repos/herdr")
APP = "Herdr Shell.app"
STORE = os.environ.get("HERDR_SHELL_RELEASE_DIR", f"{HOME}/.local/share/herdr-shell/release")
TARGETS = os.environ.get("HERDR_SHELL_TARGETS", f"{HOME}/.config/herdr-shell/targets.json")
LOGDIR = f"{HOME}/.cache/herdr-shell-publish"
LOCK = f"{LOGDIR}/publish.lock"
RELEASES = f"{HOME}/Library/Caches/herdr-shell-publish/releases"
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


@contextmanager
def delivery_lock():
    os.makedirs(LOGDIR, exist_ok=True)
    with open(f"{LOGDIR}/delivery.lock", "a") as lk:
        fcntl.flock(lk, fcntl.LOCK_EX)
        yield


def serialized_delivery(fn):
    @wraps(fn)
    def locked(*args, **kwargs):
        with delivery_lock():
            return fn(*args, **kwargs)
    return locked


def git(*a):
    return subprocess.run(["git", "-C", REPO, *a], capture_output=True, text=True, check=True).stdout.strip()


def branch():
    r = subprocess.run(["git", "-C", REPO, "config", "herdr-shell.releaseBranch"], capture_output=True, text=True)
    return r.stdout.strip() or "main"


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


def shell_changed(old, new):
    """True unless macos/HerdrShell is identical at both commits; an unknown commit counts
    as changed. Most pushes to main touch only the Rust side and need no new app."""
    r = subprocess.run(["git", "-C", REPO, "diff", "--quiet", old, new, "--", "macos/HerdrShell"],
                       capture_output=True)
    return r.returncode != 0


def publish(ref):
    sha = git("rev-parse", "--verify", ref + "^{commit}")
    log(f"build {ref} = {sha[:12]}")
    # release.sh comes from the commit being built: check it out in the release worktree first.
    wt = release_worktree()
    subprocess.run(["git", "-C", wt, "checkout", "-q", "--detach", sha], check=True)
    script = f"{wt}/macos/HerdrShell/scripts/release.sh"
    # release.sh stages Studio itself; hold the delivery lock through the build and the read of
    # what it staged, so a local (Studio) fanout never swaps staged/ in between.
    with open(f"{LOGDIR}/build.log", "a") as out, delivery_lock():
        r = subprocess.run(["bash", script, sha], stdout=out, stderr=subprocess.STDOUT,
                           env={**os.environ, "HERDR_REPO": REPO})
        if r.returncode != 0:
            log(f"FAIL release.sh {sha[:12]} exit {r.returncode} (see {LOGDIR}/build.log)")
            return False
        try:
            with open(f"{STAGE}/staged.json") as f:
                meta = json.load(f)
        except (OSError, ValueError) as e:
            log(f"FAIL staged metadata {sha[:12]}: {e}")
            return False
    if not same(meta.get("commit", ""), sha):
        log(f"FAIL staged.json names {meta.get('commit')} after building {sha[:12]}")
        return False
    # Snapshot the build output, never Studio's consumable Update staging directory.
    os.makedirs(RELEASES, exist_ok=True)
    snapshot = f"{RELEASES}/{sha}"
    tmp = tempfile.mkdtemp(prefix=f".{sha}.", dir=RELEASES)
    try:
        subprocess.run(["ditto", f"{wt}/macos/HerdrShell/.build/bundle-prod/{APP}", f"{tmp}/{APP}"], check=True)
        with open(f"{tmp}/release.json", "w") as f:
            json.dump(meta, f, indent=2)
            f.write("\n")
        with delivery_lock():
            if not os.path.isdir(snapshot):
                os.rename(tmp, snapshot)
            os.makedirs(os.path.dirname(STORE), exist_ok=True)
            link = STORE + ".incoming"
            if os.path.lexists(link):
                if os.path.islink(link):
                    os.unlink(link)
                else:
                    shutil.rmtree(link)
            os.symlink(snapshot, link)
            if os.path.isdir(STORE) and not os.path.islink(STORE):
                old = STORE + ".previous"
                shutil.rmtree(old, ignore_errors=True)
                os.rename(STORE, old)
            os.replace(link, STORE)
    except (OSError, subprocess.CalledProcessError) as e:
        log(f"FAIL snapshot {sha[:12]}: {e}")
        return False
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    log(f"release {meta['commit']} staged on studio")
    fanout()
    return True


REMOTE_PROBE = r'''
S="$HOME/Library/Application Support/HerdrShell"
i=$(/usr/libexec/PlistBuddy -c "Print :HerdrShellCommit" "$HOME/Applications/Herdr Shell.app/Contents/Info.plist" 2>/dev/null)
s=$(/usr/bin/plutil -extract commit raw -o - "$S/staged.json" 2>/dev/null)
a=$(/usr/libexec/PlistBuddy -c "Print :HerdrShellCommit" "$S/staged/Herdr Shell.app/Contents/Info.plist" 2>/dev/null)
c=$(/usr/bin/mdfind "kMDItemCFBundleIdentifier == 'com.aneyman.herdr-shell'" 2>/dev/null | wc -l | tr -d ' ')
echo "installed=$i"; echo "staged=$s"; echo "staged_app=$a"; echo "spotlight_copies=$c"
'''

# Raycast and Spotlight list one app per bundle id, and Herdr Shell vanished from them
# behind seven staged copies left in Application Support. Keep only the current staged
# release, inside a .noindex folder Spotlight skips, and out of LaunchServices.
PRUNE_STAGED = r'''
LSR=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
R="$S/releases.noindex"
keep=$(readlink "$S/staged" 2>/dev/null || true)
for old in "$S"/releases/incoming.* "$R"/incoming.* "$S"/prev-*.app; do
    [ -e "$old" ] || [ -L "$old" ] || continue
    [ "$old" = "$keep" ] && continue
    find "$old" -maxdepth 2 -name '*.app' -prune -exec "$LSR" -u {} \; 2>/dev/null || true
    rm -rf "$old"
done
rmdir "$S/releases" 2>/dev/null || true
[ -d "$keep" ] && { find "$keep" -maxdepth 2 -name '*.app' -prune -exec "$LSR" -u {} \; 2>/dev/null || true; }
'''

# Both files live behind one staged directory pointer, swapped only after verification.
REMOTE_DELIVER = r'''
set -e
S="$HOME/Library/Application Support/HerdrShell"
mkdir -p "$S/releases.noindex"
IN=$(mktemp -d "$S/releases.noindex/incoming.XXXXXX")
trap 'rm -rf "$IN"' EXIT
tar -xzf - -C "$IN"
/usr/bin/codesign --verify "$IN/Herdr Shell.app"
# Migrate the old layout once; subsequent deliveries replace a symlink atomically.
if [ -d "$S/staged" ] && [ ! -L "$S/staged" ]; then
    mv "$S/staged" "$IN/legacy"
fi
if [ ! -L "$S/staged.json" ]; then
    rm -f "$S/staged.json"
    ln -s staged/release.json "$S/staged.json"
fi
LINK="$IN.pointer"
ln -s "$IN" "$LINK"
/bin/mv -fh "$LINK" "$S/staged"
trap - EXIT
''' + PRUNE_STAGED + r'''
echo delivered
'''


def on(t, script):
    """argv that runs script on target t: over ssh, or here for a "local" target (Studio)."""
    if t.get("local"):
        return ["/bin/sh", "-c", script]
    return t["ssh"][:1] + SSH_OPTS + t["ssh"][1:] + ["/bin/sh", "-c", shlex.quote(script)]


def probe(t):
    r = subprocess.run(on(t, REMOTE_PROBE),
                       capture_output=True, text=True, timeout=60)
    if r.returncode != 0:
        return None
    return dict(l.split("=", 1) for l in r.stdout.splitlines() if "=" in l)


@serialized_delivery
def fanout():
    rel = release()
    if not rel:
        log("fanout: no release yet")
        return
    commit = rel["commit"]
    source = os.path.realpath(STORE)
    for t in targets():
        name = t.get("name", "?")
        try:
            st = probe(t)
        except (OSError, subprocess.TimeoutExpired):
            st = None
        if st is None:
            log(f"{name}: unreachable, will retry")
            continue
        if t.get("server") and not t.get("local"):
            send_server_config(t)
        if same(st.get("installed", ""), commit):
            continue
        if same(st.get("staged", ""), commit) and same(st.get("staged_app", ""), commit):
            continue
        if not os.path.isdir(f"{source}/{APP}") or not os.path.isfile(f"{source}/release.json"):
            log(f"{name}: FAIL deliver {commit}: release source missing")
            continue
        tar = subprocess.Popen(["tar", "-czf", "-", "-C", source, APP, "release.json"], stdout=subprocess.PIPE)
        try:
            r = subprocess.run(on(t, REMOTE_DELIVER),
                               stdin=tar.stdout, capture_output=True, text=True, timeout=900)
        except (OSError, subprocess.TimeoutExpired) as e:
            log(f"{name}: FAIL deliver {commit}: {e}")
            r = None
        finally:
            tar.stdout.close()
            tar_code = tar.wait()
        if r is not None and r.returncode == 0 and tar_code == 0 and "delivered" in r.stdout:
            log(f"{name}: staged {commit} (installed {st.get('installed') or 'none'})")
        elif r is not None:
            log(f"{name}: FAIL deliver {commit}: {(r.stderr or r.stdout).strip()[-300:]}")



# A target's "server" entry is how its app reaches the herdr server for Park, Resume and
# Approve (RemoteActions). Written once; a hand-edited server.json on the target wins.
REMOTE_SERVER = r'''
C="$HOME/.config/herdr-shell/server.json"
[ -e "$C" ] && { echo kept; exit 0; }
mkdir -p "$HOME/.config/herdr-shell"
T="$C.tmp.$$"
cat > "$T" || { rm -f "$T"; exit 1; }
# ln never replaces: a server.json made since the check above wins.
if ln "$T" "$C" 2>/dev/null; then echo written; else echo kept; fi
rm -f "$T"
'''


def send_server_config(t):
    try:
        r = subprocess.run(on(t, REMOTE_SERVER),
                           input=json.dumps(t["server"]), capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired) as e:
        log(f"{t.get('name', '?')}: FAIL server config: {e}")
        return
    if "written" in r.stdout:
        log(f"{t.get('name', '?')}: wrote server.json")
    elif r.returncode != 0:
        log(f"{t.get('name', '?')}: FAIL server config: {(r.stderr or r.stdout).strip()[-200:]}")


REMOTE_INSTALL = r'''
set -e
if pgrep -x HerdrShell >/dev/null; then echo running; exit 3; fi
A="$HOME/Applications"
mkdir -p "$A"
IN="$A/.herdr-shell-incoming"
rm -rf "$IN"; mkdir "$IN"
tar -xzf - -C "$IN"
/usr/bin/codesign --verify "$IN/Herdr Shell.app"
rm -rf "$A/Herdr Shell.app.previous"
[ -d "$A/Herdr Shell.app" ] && mv "$A/Herdr Shell.app" "$A/Herdr Shell.app.previous"
mv "$IN/Herdr Shell.app" "$A/Herdr Shell.app"
rm -rf "$IN" "$A/Herdr Shell.app.previous"
S="$HOME/Library/Application Support/HerdrShell"
# A staged release from the old layout moves under .noindex; the app is not running.
cur=$(readlink "$S/staged" 2>/dev/null || true)
case "$cur" in "$S"/releases/incoming.*)
    mkdir -p "$S/releases.noindex"
    new="$S/releases.noindex/${cur##*/}"
    mv "$cur" "$new"
    ln -s "$new" "$new.pointer"
    /bin/mv -fh "$new.pointer" "$S/staged";;
esac
''' + PRUNE_STAGED + r'''
"$LSR" -f "$A/Herdr Shell.app" || true
/usr/bin/mdimport "$A/Herdr Shell.app" 2>/dev/null || true
echo installed
'''


@serialized_delivery
def install(name):
    rel = release()
    t = next((t for t in targets() if t.get("name") == name), None)
    if not rel or not t:
        sys.exit(f"install: no release or no target {name}")
    source = os.path.realpath(STORE)
    if not os.path.isdir(f"{source}/{APP}"):
        sys.exit(f"{name}: install failed: release source missing")
    tar = subprocess.Popen(["tar", "-czf", "-", "-C", source, APP], stdout=subprocess.PIPE)
    r = subprocess.run(on(t, REMOTE_INSTALL),
                       stdin=tar.stdout, capture_output=True, text=True, timeout=900)
    tar.stdout.close()
    tar.wait()
    if r.returncode == 0 and "installed" in r.stdout:
        log(f"{name}: installed {rel['commit']}")
    else:
        sys.exit(f"{name}: install failed: {(r.stdout + r.stderr).strip()[-300:]}")


DATA_DIR = f"{HOME}/.agent-rails/herdr"
DATA_FILES = ("lanes.json", "areas.json", "modes.json", "overlay.json")

REMOTE_DATA = r'''
set -e
D="$HOME/.agent-rails/herdr"
mkdir -p "$D"
IN=$(mktemp -d "$D/.incoming.XXXXXX")
tar -xf - -C "$IN"
for f in "$IN"/*.json; do mv "$f" "$D/$(basename "$f")"; done
rmdir "$IN"
echo synced
'''


@serialized_delivery
def data():
    """Push the Areas/Parked inputs to each target when their content changed."""
    have = [f for f in DATA_FILES if os.path.isfile(f"{DATA_DIR}/{f}")]
    if not have:
        return
    # Hash and send the same snapshot even if Studio updates a file during delivery.
    with tempfile.TemporaryDirectory(prefix="herdr-data-", dir=LOGDIR) as snapshot:
        digests = {}
        for f in have:
            shutil.copyfile(f"{DATA_DIR}/{f}", f"{snapshot}/{f}")
            with open(f"{snapshot}/{f}", "rb") as fh:
                digests[f] = hashlib.sha256(fh.read()).hexdigest()
        send_data(snapshot, digests)


def send_data(snapshot, digests):
    state_path = f"{LOGDIR}/data-state.json"
    try:
        with open(state_path) as fh:
            state = json.load(fh)
    except (OSError, ValueError):
        state = {}
    for t in targets():
        if t.get("local"):
            continue  # the data files already live here
        name = t.get("name", "?")
        sent = state.get(name)
        if not isinstance(sent, dict):
            sent = {}
        changed = [f for f, digest in digests.items() if sent.get(f) != digest]
        if not changed:
            continue
        tar = subprocess.Popen(["tar", "-cf", "-", "-C", snapshot, *changed], stdout=subprocess.PIPE)
        try:
            r = subprocess.run(on(t, REMOTE_DATA),
                               stdin=tar.stdout, capture_output=True, text=True, timeout=60)
        except (OSError, subprocess.TimeoutExpired):
            r = None
        tar.stdout.close()
        tar_code = tar.wait()
        if r is not None and r.returncode == 0 and tar_code == 0 and "synced" in r.stdout:
            state[name] = {**sent, **{f: digests[f] for f in changed}}
            state.pop(f"{name}.down", None)
        elif not state.get(f"{name}.down"):
            state[f"{name}.down"] = True
            log(f"{name}: data sync failed, will retry quietly")
    os.makedirs(LOGDIR, exist_ok=True)
    with open(state_path + ".tmp", "w") as fh:
        json.dump(state, fh)
    os.replace(state_path + ".tmp", state_path)


def fetch():
    """Bring origin/<release branch> up to date. The hook skips this ref update; the caller
    runs `auto` itself."""
    subprocess.run(["git", "-C", REPO, "fetch", "-q", "origin", branch()],
                   capture_output=True, timeout=60, env={**os.environ, "HERDR_SHELL_PUBLISHING": "1"})


QUIET = float(os.environ.get("HERDR_SHELL_QUIET", "20"))
QUIET_MAX = 180


def settled(ref):
    """The branch tip once it has not moved for QUIET seconds (a burst of pushes builds once),
    refetching every few seconds. Gives up waiting after QUIET_MAX and takes the tip then."""
    sha, since, start = git("rev-parse", "--verify", ref), time.time(), time.time()
    while time.time() - since < QUIET and time.time() - start < QUIET_MAX:
        time.sleep(min(5, QUIET))
        try:
            fetch()
        except (OSError, subprocess.TimeoutExpired):
            pass
        now = git("rev-parse", "--verify", ref)
        if now != sha:
            sha, since = now, time.time()
    return sha


def auto(refetch=False):
    os.makedirs(LOGDIR, exist_ok=True)
    with open(LOCK, "w") as lk:
        try:
            fcntl.flock(lk, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            # A publish is running; it re-reads the branch when it finishes.
            return
        if refetch:
            try:
                fetch()
            except (OSError, subprocess.TimeoutExpired):
                pass
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
            if rel and not shell_changed(rel.get("commit", ""), sha):
                # Re-read the branch: a push that lost the lock to this run is still ours.
                continue
            quiet = settled(ref)
            if quiet != sha:
                continue
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
    elif a and a[0] == "watch":
        auto(refetch=True)
    elif a and a[0] == "fanout":
        fanout()
    elif a and a[0] == "install" and len(a) == 2:
        install(a[1])
    elif a and a[0] == "data":
        data()
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
