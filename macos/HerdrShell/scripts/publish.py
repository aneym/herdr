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
import json
import os
import plistlib
import shutil
import subprocess
import sys
import time
import tempfile
from publish_fetch import shared_fetch
from publish_build_lock import studio_build_lock
from publish_data import data
from publish_transport import REMOTE_DELIVER, REMOTE_INSTALL, on, probe, send_server_config

HOME = os.path.expanduser("~")
REPO = os.environ.get("HERDR_REPO", "/Volumes/StudioExt/repos/herdr")
APP = "Herdr Shell.app"
STORE = os.environ.get("HERDR_SHELL_RELEASE_DIR", f"{HOME}/.local/share/herdr-shell/release")
TARGETS = os.environ.get("HERDR_SHELL_TARGETS", f"{HOME}/.config/herdr-shell/targets.json")
LOGDIR = f"{HOME}/.cache/herdr-shell-publish"
LOCK = f"{LOGDIR}/publish.lock"
RELEASES = f"{HOME}/Library/Caches/herdr-shell-publish/releases"

KEEP_RELEASES = int(os.environ.get("HERDR_SHELL_KEEP_RELEASES", "5"))
STAGE = f"{HOME}/Library/Application Support/HerdrShell"

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


def verify_release(source, commit):
    """Never ship a bundle whose identity differs from its release metadata."""
    try:
        with open(f"{source}/{APP}/Contents/Info.plist", "rb") as f:
            built = plistlib.load(f).get("HerdrShellCommit")
    except (OSError, ValueError, plistlib.InvalidFileException) as e:
        sys.exit(f"release source invalid: {e}")
    if not same(built, commit):
        sys.exit(f"release bundle commit mismatch: expected {commit}, got {built}")


def targets():
    try:
        with open(TARGETS) as f:
            return json.load(f).get("targets", [])
    except (OSError, ValueError):
        return []


def same(a, b):
    return (isinstance(a, str) and isinstance(b, str) and min(len(a), len(b)) >= 12
            and (a.startswith(b) or b.startswith(a)))


def can_deliver(installed, commit, name):
    """Fail closed unless the release is a strict descendant of a known install."""
    if not installed:
        log(f"{name}: skip {commit}: installed commit unknown")
        return False
    if same(installed, commit):
        return False
    for attempt in range(2):
        try:
            old = git("rev-parse", "--verify", installed + "^{commit}")
            new = git("rev-parse", "--verify", commit + "^{commit}")
            break
        except subprocess.CalledProcessError:
            if attempt == 0:
                try:
                    fetch()
                except (OSError, subprocess.TimeoutExpired):
                    pass
            else:
                log(f"{name}: skip {commit}: unknown installed/release commit {installed}")
                return False
    if not is_ancestor(old, new):
        twin = tree_twin(old, new)
        if twin:
            log(f"{name}: installed {installed} was rewritten as {twin[:12]} (same tree)")
            old = twin
    if old != new and is_ancestor(old, new):
        return True
    if old != new:
        log(f"{name}: skip {commit}: not a strict descendant of installed {installed}")
        stalled(name, f"installed {installed} is not an ancestor of the release")
    return False


def is_ancestor(old, new):
    return subprocess.run(["git", "-C", REPO, "merge-base", "--is-ancestor", old, new],
                          capture_output=True).returncode == 0


def tree_twin(old, new):
    """The commit in new's history, outside old's, with old's exact tree, so an install whose
    commit was later rebased or amended onto the branch counts as that branch commit; None if
    there is none. Commits old already contains never match, so an older release stays refused."""
    tree = git("rev-parse", old + "^{tree}")
    since = int(git("log", "-1", "--format=%ct", old)) - 86400
    for line in git("log", "--max-count=500", f"--since={since}", "--format=%H %T", new, "^" + old).splitlines():
        sha, t = line.split()
        if t == tree:
            return sha
    return None


def stalled(name, reason):
    """Record why a target is stuck behind the release in stalled.json, which `status` prints,
    so a skip that repeats every watch run is visible outside the log."""
    path = f"{LOGDIR}/stalled.json"
    try:
        with open(path) as f:
            state = json.load(f)
    except (OSError, ValueError):
        state = {}
    if reason is None and name not in state:
        return
    if reason is None:
        state.pop(name)
    elif state.get(name, {}).get("reason") == reason:
        return
    else:
        state[name] = {"reason": reason, "since": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    os.makedirs(LOGDIR, exist_ok=True)
    with open(path + ".tmp", "w") as f:
        json.dump(state, f)
    os.replace(path + ".tmp", path)


def shell_changed(old, new):
    """True unless macos/HerdrShell is identical at both commits; an unknown commit counts
    as changed. Most pushes to main touch only the Rust side and need no new app."""
    r = subprocess.run(["git", "-C", REPO, "diff", "--quiet", old, new, "--", "macos/HerdrShell"],
                       capture_output=True)
    return r.returncode != 0


def prune_releases():
    """Keep the newest KEEP_RELEASES snapshots and whatever the release points at.
    Called under the delivery lock, so no fanout or install is reading an old one."""
    current = os.path.realpath(STORE)
    snaps = [e for e in os.scandir(RELEASES) if e.is_dir(follow_symlinks=False) and not e.name.startswith(".")]
    snaps.sort(key=lambda e: e.stat(follow_symlinks=False).st_mtime, reverse=True)
    for e in snaps[KEEP_RELEASES:]:
        if os.path.realpath(e.path) != current:
            shutil.rmtree(e.path, ignore_errors=True)
            log(f"pruned release {e.name[:12]}")


def publish(ref):
    sha = git("rev-parse", "--verify", ref + "^{commit}")
    log(f"build {ref} = {sha[:12]}")

    wt = release_worktree()
    subprocess.run(["git", "-C", wt, "checkout", "-q", "--detach", sha], check=True)
    script = f"{wt}/macos/HerdrShell/scripts/release.sh"


    with open(f"{LOGDIR}/build.log", "a") as out, studio_build_lock(log), delivery_lock():
        st = probe({"local": True})
        if st is None or st.get("installed_app") not in ("0", "1"):
            log("studio build: installed app state unknown")
            return False
        if same(st.get("installed", ""), sha):
            log(f"studio build: already installed {sha[:12]}")
            return True
        if st.get("installed") or st.get("installed_app") == "1":
            if not can_deliver(st.get("installed", ""), sha, "studio build"):
                return False
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
            else:
                os.utime(snapshot)
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
            prune_releases()
    except (OSError, subprocess.CalledProcessError) as e:
        log(f"FAIL snapshot {sha[:12]}: {e}")
        return False
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    log(f"release {meta['commit']} staged on studio")
    fanout()
    return True


@serialized_delivery
def fanout():
    rel = release()
    if not rel:
        log("fanout: no release yet")
        return
    commit = rel["commit"]
    source = os.path.realpath(STORE)
    verify_release(source, commit)
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
            send_server_config(t, log)
        if not can_deliver(st.get("installed", ""), commit, name):
            continue
        if same(st.get("staged", ""), commit) and same(st.get("staged_app", ""), commit):
            stalled(name, None)
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
            stalled(name, None)
            log(f"{name}: staged {commit} (installed {st.get('installed') or 'none'})")
        elif r is not None:
            log(f"{name}: FAIL deliver {commit}: {(r.stderr or r.stdout).strip()[-300:]}")


@serialized_delivery
def install(name):
    rel = release()
    t = next((t for t in targets() if t.get("name") == name), None)
    if not rel or not t:
        sys.exit(f"install: no release or no target {name}")
    st = probe(t)
    if st is None:
        sys.exit(f"{name}: install refused: target unreachable")
    if st.get("installed_app") not in ("0", "1"):
        sys.exit(f"{name}: install refused: installed app state unknown")
    if st.get("installed") or st.get("installed_app") == "1":
        if not can_deliver(st.get("installed", ""), rel["commit"], name):
            sys.exit(f"{name}: install refused: release is not a known strict descendant")
    source = os.path.realpath(STORE)
    verify_release(source, rel["commit"])
    if not os.path.isdir(f"{source}/{APP}"):
        sys.exit(f"{name}: install failed: release source missing")
    tar = subprocess.Popen(["tar", "-czf", "-", "-C", source, APP], stdout=subprocess.PIPE)
    r = subprocess.run(on(t, REMOTE_INSTALL),
                       stdin=tar.stdout, capture_output=True, text=True, timeout=900)
    tar.stdout.close()
    tar_code = tar.wait()
    if r.returncode == 0 and tar_code == 0 and "installed" in r.stdout:
        stalled(name, None)
        log(f"{name}: installed {rel['commit']}")
    else:
        sys.exit(f"{name}: install failed: {(r.stdout + r.stderr).strip()[-300:]}")


def fetch(max_age=60):
    shared_fetch(REPO, branch(), LOGDIR, max_age=max_age)


QUIET = float(os.environ.get("HERDR_SHELL_QUIET", "20"))
QUIET_MAX = 180


def settled(ref):
    """The branch tip once it has not moved for QUIET seconds (a burst of pushes builds once),
    refetching every few seconds. Gives up waiting after QUIET_MAX and takes the tip then."""
    sha, since, start = git("rev-parse", "--verify", ref), time.time(), time.time()
    while time.time() - since < QUIET and time.time() - start < QUIET_MAX:
        time.sleep(min(5, QUIET))
        try:
            fetch(max_age=0)
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

            return
        if refetch:
            try:
                fetch()
            except (OSError, subprocess.TimeoutExpired):
                pass
        checked_path = os.path.join(LOGDIR, "checked-target.json")
        try:
            with open(checked_path) as source:
                checked = json.load(source)
        except (OSError, ValueError):
            checked = None
        tried = set()
        while True:
            ref = f"refs/remotes/origin/{branch()}"
            try:
                sha = git("rev-parse", "--verify", ref)
            except subprocess.CalledProcessError:
                log(f"auto: no {ref}")
                return
            rel = release()
            target = {"repo": os.path.realpath(REPO), "ref": ref, "sha": sha, "release": rel and rel.get("commit")}
            if checked == target or (rel and same(rel.get("commit", ""), sha)) or sha in tried:
                return
            tried.add(sha)
            if rel and not shell_changed(rel.get("commit", ""), sha):
                with open(checked_path + ".tmp", "w") as destination:
                    json.dump(target, destination)
                os.replace(checked_path + ".tmp", checked_path)
                checked = target
                continue
            quiet = settled(ref)
            if quiet != sha:
                sha = quiet
                if (rel and same(rel.get("commit", ""), sha)) or sha in tried:
                    return
                tried.add(sha)
                if rel and not shell_changed(rel.get("commit", ""), sha):
                    continue
            publish(sha)


def status():
    rel = release()
    print(json.dumps({"branch": branch(), "release": rel and rel.get("commit")}))
    try:
        with open(f"{LOGDIR}/stalled.json") as f:
            for name, why in json.load(f).items():
                print(json.dumps({"target": name, "STALLED": why}))
    except (OSError, ValueError):
        pass
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
        with delivery_lock():
            data(LOGDIR, targets(), log)
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
