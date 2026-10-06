#!/usr/bin/env python3
"""Dev reload check: a burst of pushes builds once, and the app swaps itself.

  check_dev_reload.py              publish side only: real git repos in a scratch dir,
                                   publish.py `watch` against a bare origin, a stub
                                   release.sh that records each build. No app, no window.
  check_dev_reload.py --space      also the app side, in the Cua Space (herdr-shell-space,
                                   --live, nothing typed): build the prod bundle at HEAD,
                                   stage a copy stamped with another commit, and wait for
                                   the running app to relaunch into it by itself.
                                   Screenshots go to --evidence DIR.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PUBLISH = os.path.join(D0, "scripts", "publish.py")
lines, failures = [], []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def git(cwd, *a):
    return subprocess.run(["git", "-C", cwd, *a], capture_output=True, text=True, check=True).stdout.strip()


STUB_RELEASE = r'''#!/bin/bash
# Stand-in for release.sh: record the build, stage metadata the way the real one does.
set -euo pipefail
SHA="$1"
WT="$(cd "$(dirname "$0")/../../.." && pwd)"
echo "$SHA" >> "$HOME/builds.txt"
mkdir -p "$WT/macos/HerdrShell/.build/bundle-prod/Herdr Shell.app/Contents"
S="$HOME/Library/Application Support/HerdrShell"
mkdir -p "$S"
printf '{"commit": "%s", "built_at": "x", "ref": "%s", "notes": []}\n' "$SHA" "$SHA" > "$S/staged.json"
'''


def commit(repo, path, text, msg):
    full = os.path.join(repo, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "w") as f:
        f.write(text)
    git(repo, "add", "-A")
    git(repo, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", msg)
    git(repo, "push", "-q", "origin", "HEAD:main")
    return git(repo, "rev-parse", "HEAD")


def publish_side():
    tmp = tempfile.mkdtemp(prefix="devreload-")
    try:
        home, origin, seed = (os.path.join(tmp, n) for n in ("home", "origin.git", "seed"))
        repo = os.path.join(tmp, "repos", "herdr")
        os.makedirs(home)
        subprocess.run(["git", "init", "-q", "--bare", "-b", "main", origin], check=True)
        subprocess.run(["git", "clone", "-q", origin, seed], check=True, capture_output=True)
        stub = os.path.join(seed, "macos/HerdrShell/scripts/release.sh")
        os.makedirs(os.path.dirname(stub))
        with open(stub, "w") as f:
            f.write(STUB_RELEASE)
        os.chmod(stub, 0o755)
        commit(seed, "README", "seed\n", "seed")
        subprocess.run(["git", "clone", "-q", origin, repo], check=True, capture_output=True)
        env = {**os.environ, "HOME": home, "HERDR_REPO": repo, "HERDR_SHELL_QUIET": "3",
               "HERDR_SHELL_RELEASE_DIR": os.path.join(home, "release"),
               "HERDR_SHELL_TARGETS": os.path.join(home, "targets.json")}

        def watch(background=False):
            argv = [sys.executable, PUBLISH, "watch"]
            if background:
                return subprocess.Popen(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            return subprocess.run(argv, env=env, capture_output=True, text=True, timeout=300)

        def builds():
            try:
                return open(os.path.join(home, "builds.txt")).read().split()
            except OSError:
                return []

        # A push made from another clone reaches the repo only through watch's fetch.
        first = commit(seed, "macos/HerdrShell/a.txt", "1\n", "shell 1")
        watch()
        check("watch publishes a push made elsewhere", builds() == [first], f"builds {builds()}")

        # A burst: three Shell pushes a second apart while the first waits for quiet.
        c1 = commit(seed, "macos/HerdrShell/a.txt", "2\n", "shell 2")
        p = watch(background=True)
        time.sleep(1)
        commit(seed, "macos/HerdrShell/a.txt", "3\n", "shell 3")
        time.sleep(1)
        c3 = commit(seed, "macos/HerdrShell/a.txt", "4\n", "shell 4")
        t0 = time.time()
        p.wait(timeout=120)
        got = builds()[1:]
        check("a burst of pushes builds once, at the last commit", got == [c3], f"builds {got} (c1 {c1[:8]}, c3 {c3[:8]})")
        check("the build waits for the quiet window", time.time() - t0 >= 2.5, f"{time.time() - t0:.1f} s after the last push")

        # A push that leaves macos/HerdrShell alone builds nothing.
        commit(seed, "src/main.rs", "fn main() {}\n", "rust only")
        watch()
        check("a push without Shell changes builds nothing", len(builds()) == 2, f"builds {len(builds())}")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def space(cmd, *a, check_ok=True):
    # One run at a time: start waits for the Space lock (up to 25 min) under this owner name.
    env = {**os.environ, "HERDR_SPACE_OWNER": os.environ.get("HERDR_SPACE_OWNER", "check_dev_reload")}
    r = subprocess.run(["herdr-shell-space", cmd, *a], capture_output=True, text=True, timeout=1800, env=env)
    if check_ok and r.returncode != 0:
        raise RuntimeError(f"herdr-shell-space {cmd}: {r.stderr.strip()[-400:]}")
    return r.stdout.strip()


GS = "$HOME/Library/Application Support/HerdrShell"
STAGED_COMMIT = "d0e1b2c3d4e5"


def app_side(evidence):
    os.makedirs(evidence, exist_ok=True)
    out = os.path.join(os.path.expanduser("~/.cache/herdr-build"), "devreload")
    a_dir, b_dir = os.path.join(out, "A"), os.path.join(out, "B")
    for d in (a_dir, b_dir):
        os.makedirs(d, exist_ok=True)
    build = subprocess.run(["nice", "-n", "10", "swift", "build", "-c", "release"], cwd=D0, capture_output=True, text=True)
    check("swift build", build.returncode == 0, build.stderr[-300:] if build.returncode else "clean")
    if build.returncode:
        return
    a_app = subprocess.check_output(["bash", os.path.join(D0, "scripts", "bundle.sh"), "prod", a_dir], text=True).strip().splitlines()[-1]
    b_app = os.path.join(b_dir, "Herdr Shell.app")
    shutil.rmtree(b_app, ignore_errors=True)
    subprocess.run(["ditto", a_app, b_app], check=True)
    subprocess.run(["/usr/libexec/PlistBuddy", "-c", f"Set :HerdrShellCommit {STAGED_COMMIT}",
                    os.path.join(b_app, "Contents", "Info.plist")], check=True)
    subprocess.run(["codesign", "-s", "-", "--force", "--deep", b_app], check=True, capture_output=True)

    started = json.loads(space("start", "--wait", "1500", "--app", a_app, "--live").splitlines()[-1])
    # With the lock held: no staged release, no earlier tries, dev reload on (read live by the app).
    space("exec", f'S="{GS}"; rm -rf "$S/staged" "$S/staged.json" "$S/update.log" "$S/reload.json" "$S/reload-result.json"; '
                  "defaults delete com.aneyman.herdr-shell herdr.shell.autoUpdateTried 2>/dev/null; "
                  "defaults write com.aneyman.herdr-shell herdr.shell.devReload -bool true")
    old_pid = started["app_pid"]
    time.sleep(6)
    space("shot", os.path.join(evidence, "shell-dev-reload-1-before.png"))

    # Stage the other build the way publish delivers it: the app first, staged.json last.
    mod = load_space_module()
    mod.push_tree(b_app, mod.GUEST_HOME + "/Library/Application Support/HerdrShell/staged/Herdr Shell.app")
    meta = json.dumps({"commit": STAGED_COMMIT, "built_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                       "ref": "check_dev_reload", "notes": ["dev reload check"]})
    space("exec", f"printf '%s' '{meta}' > \"{GS}/staged.json\"")
    staged_at = time.time()

    result, shot_taken = None, False
    while time.time() - staged_at < 90:
        raw = space("exec", f'cat "{GS}/reload-result.json" 2>/dev/null', check_ok=False)
        if raw and not shot_taken:
            space("shot", os.path.join(evidence, "shell-dev-reload-2-toast.png"))
            shot_taken = True
        if raw:
            result = json.loads(raw)
            break
        time.sleep(0.5)
    applied_s = time.time() - staged_at
    installed = space("exec", '/usr/libexec/PlistBuddy -c "Print :HerdrShellCommit" "$HOME/Applications/Herdr Shell.app/Contents/Info.plist"')
    pids = space("exec", "pgrep -x HerdrShell || true", check_ok=False).split()
    time.sleep(5)
    space("shot", os.path.join(evidence, "shell-dev-reload-3-after.png"))
    with open(os.path.join(evidence, "shell-dev-reload-result.json"), "w") as f:
        json.dump({"result": result, "installed": installed, "pids": pids, "old_pid": old_pid,
                   "staged_to_settled_s": applied_s}, f, indent=2)

    check("the running app applied the staged build by itself", result is not None, f"{applied_s:.1f} s after staging")
    check("the new bundle is installed", installed == STAGED_COMMIT, installed)
    check("the old process is gone, one new one runs", old_pid not in pids and len(pids) == 1, f"pids {pids}, old {old_pid}")
    if result:
        check("selected tab restored", result["selected_tab"] == result["selected_tab_before"] and result["selected_tab"] != "",
              f"{result['selected_tab_before']} -> {result['selected_tab']}")
        check("focused pane restored", result["focused_pane"] == result["focused_pane_before"],
              f"{result['focused_pane_before']} -> {result['focused_pane']}")
        check("window back in under 2 s", 0 <= result["window_s"] < 2, f"{result['window_s']:.2f} s")
        say(f"swap: window {result['window_s']:.2f} s, panes {result['panes_s']:.2f} s after the old app quit")
    space("exec", "defaults delete com.aneyman.herdr-shell herdr.shell.devReload", check_ok=False)
    space("stop", check_ok=False)


def load_space_module():
    import importlib.machinery
    import importlib.util
    path = shutil.which("herdr-shell-space")
    loader = importlib.machinery.SourceFileLoader("herdr_shell_space", path)
    spec = importlib.util.spec_from_loader("herdr_shell_space", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def main():
    publish_side()
    if "--space" in sys.argv:
        i = sys.argv.index("--evidence") if "--evidence" in sys.argv else -1
        evidence = sys.argv[i + 1] if i >= 0 else os.path.join(D0, "checks")
        app_side(evidence)
    say(f"{'PASS' if not failures else 'FAIL'}: {len(failures)} failing")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
