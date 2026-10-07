#!/usr/bin/env python3
"""CLI regression: cached releases cannot overwrite newer or unknown installs.

Uses real git histories, plist probing and the publisher CLI. No existing test
covers delivery ancestry; a stale-cache regression must skip before staging.
The SSH and Swift build edges are isolated; no production functions are mocked.
"""
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

publisher = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).with_name("publish.py")
case = os.environ.get("PUBLISH_CHECK_CASE", "all")
release_script = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else Path(__file__).with_name("release.sh")
with tempfile.TemporaryDirectory(prefix="publish-check-", dir=os.environ.get("TMPDIR")) as tmp:
    root = Path(tmp)
    repo = root / "repo"
    repo.mkdir()

    def git(*args):
        return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()

    git("init", "-q", "-b", "main")
    git("config", "user.name", "Publisher check")
    git("config", "user.email", "publisher-check@example.invalid")
    git("commit", "--allow-empty", "-qm", "old")
    old = git("rev-parse", "HEAD")
    git("commit", "--allow-empty", "-qm", "new")
    new = git("rev-parse", "HEAD")
    git("remote", "add", "origin", str(repo))
    home = root / "home"
    plist = home / "Applications/Herdr Shell.app/Contents/Info.plist"
    plist.parent.mkdir(parents=True)
    store = root / "release"
    store.mkdir()
    (store / "release.json").write_text(json.dumps({"commit": old}))
    # A real signed app ensures the pre-fix code really stages the stale release;
    # a fake bundle would fail codesign and give an unrelated negative control.
    shutil.copytree(Path.home() / "Applications/Herdr Shell.app", store / "Herdr Shell.app")
    targets = root / "targets.json"
    targets.write_text(json.dumps({"targets": [{"name": "test", "local": True}]}))
    stage = home / "Library/Application Support/HerdrShell"
    stage.mkdir(parents=True)
    sentinel = stage / "staged.json"
    sentinel.write_text(json.dumps({"commit": new}))
    env = {**os.environ, "HOME": str(home), "HERDR_REPO": str(repo),
           "HERDR_SHELL_RELEASE_DIR": str(store), "HERDR_SHELL_TARGETS": str(targets)}
    bundle_plist = store / "Herdr Shell.app/Contents/Info.plist"

    def bundle_commit(commit):
        with bundle_plist.open("rb") as f:
            values = plistlib.load(f)
        values["HerdrShellCommit"] = commit
        with bundle_plist.open("wb") as f:
            plistlib.dump(values, f)
        subprocess.run(["/usr/bin/codesign", "--force", "--deep", "--sign", "-",
                        str(store / "Herdr Shell.app")], check=True, capture_output=True)

    # The remote process boundary must not see the user's running desktop app.
    # Exercise the actual remote install shell in an isolated HOME via an SSH edge.
    relay = root / "ssh_edge.py"
    # A shell function models an idle remote machine, without touching live processes.
    relay.write_text(f"#!{sys.executable}\nimport subprocess, sys, shlex\n"
                     "script = shlex.split(sys.argv[-1])[0]\n"
                     "sys.exit(subprocess.run(['/bin/sh', '-c', "
                     "'pgrep() { return 1; }; ' + script]).returncode)\n")
    relay.chmod(0o755)
    targets.write_text(json.dumps({"targets": [{"name": "test", "ssh": [str(relay)]}]}))
    bundle_commit(old)
    if case in ("all", "ancestry"):
        for installed, reason in ((new, "not a strict descendant"), ("a" * 40, "unknown installed/release commit")):
            with plist.open("wb") as f:
                plistlib.dump({"HerdrShellCommit": installed}, f)
            for command in (("fanout",), ("install", "test")):
                result = subprocess.run([sys.executable, str(publisher), *command], env=env,
                                        capture_output=True, text=True, timeout=30)
                assert result.returncode == (0 if command[0] == "fanout" else 1), result.stderr
                assert reason in result.stdout, result.stdout
                assert "FAIL deliver" not in result.stdout, result.stdout
                assert json.loads(sentinel.read_text())["commit"] == new
                with plist.open("rb") as f:
                    assert plistlib.load(f)["HerdrShellCommit"] == installed
                print(f"PASS {' '.join(command)}: {reason}; installed/staged unchanged")
    (store / "release.json").write_text(json.dumps({"commit": new}))
    with plist.open("wb") as f:
        plistlib.dump({"HerdrShellCommit": old}, f)
    if case in ("all", "mismatch-fanout", "mismatch-install"):
        for command in (("fanout",), ("install", "test")):
            if case != "all" and command[0] != case.removeprefix("mismatch-"):
                continue
            result = subprocess.run([sys.executable, str(publisher), *command], env=env,
                                    capture_output=True, text=True, timeout=30)
            assert result.returncode != 0, result.stdout
            assert "bundle commit mismatch" in result.stderr, result.stderr
            with plist.open("rb") as f:
                assert plistlib.load(f)["HerdrShellCommit"] == old
            assert json.loads(sentinel.read_text())["commit"] == new
            print(f"PASS {' '.join(command)}: bundle commit mismatch refused")

    if case in ("all", "first-install"):
        shutil.rmtree(home / "Applications/Herdr Shell.app")
        bundle_commit(new[:12])
        result = subprocess.run([sys.executable, str(publisher), "install", "test"], env=env,
                                capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, result.stdout + result.stderr
        with plist.open("rb") as f:
            assert plistlib.load(f)["HerdrShellCommit"] == new[:12]
        print("PASS install: first install delivers verified bundle")
    if case in ("all", "release", "release-first-install"):
        # Exercise release.sh against real worktrees; only the Swift build boundary is fake.
        scripts = repo / "macos/HerdrShell/scripts"
        scripts.mkdir(parents=True)
        bundle = scripts / "bundle.sh"
        bundle.write_text("#!/bin/bash\nset -e\n"
                          "python3 - \"$2\" <<'PY'\n"
                          "import os, pathlib, plistlib, shutil, subprocess, sys\n"
                          "app = pathlib.Path(sys.argv[1]) / 'Herdr Shell.app/Contents'\n"
                          "(app / 'MacOS').mkdir(parents=True, exist_ok=True)\n"
                          "shutil.copy('/usr/bin/true', app / 'MacOS/HerdrShell')\n"
                          "commit = os.environ.get('BAD_BUNDLE') or subprocess.check_output("
                          "['git', '-C', str(app.parents[4]), 'rev-parse', '--short=12', 'HEAD'], text=True).strip()\n"
                          "with (app / 'Info.plist').open('wb') as f: "
                          "plistlib.dump({'HerdrShellCommit': commit, 'HerdrShellBuiltAt': 'test'}, f)\n"
                          "PY\n")
        bundle.chmod(0o755)
        shutil.copyfile(release_script, scripts / "release.sh")
        git("add", ".")
        git("commit", "-qm", "bundle fixture")
        pinned = git("rev-parse", "HEAD")
        git("commit", "--allow-empty", "-qm", "moving ref")
        moved = git("rev-parse", "HEAD")
        git("update-ref", "refs/heads/release-test", pinned)
        edge = root / "build-edge"
        edge.mkdir()
        nice = edge / "nice"
        nice.write_text("#!/bin/bash\ngit -C \"$HERDR_REPO\" update-ref refs/heads/release-test \"$MOVED\"\n")
        nice.chmod(0o755)
        vendor = root / "Vendor"
        vendor.mkdir()
        release_home = root / "release-home"
        release_home.mkdir()
        if case == "release":
            installed_plist = release_home / "Applications/Herdr Shell.app/Contents/Info.plist"
            installed_plist.parent.mkdir(parents=True)
            with installed_plist.open("wb") as f:
                plistlib.dump({"HerdrShellCommit": old}, f)
        wt = root / "shell-release"
        # release.sh discovers its source repository from its own path.
        release_env = {**env, "HOME": str(release_home), "HERDR_RELEASE_WORKTREE": str(wt),
                       "HERDR_VENDOR": str(vendor), "MOVED": moved,
                       "PATH": str(edge) + os.pathsep + os.environ["PATH"]}
        result = subprocess.run(["bash", str(scripts / "release.sh"), "release-test"],
                                env=release_env, capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, result.stdout + result.stderr
        assert git("rev-parse", "release-test") == moved
        built_plist = release_home / "Applications/Herdr Shell.app/Contents/Info.plist"
        with built_plist.open("rb") as f:
            assert plistlib.load(f)["HerdrShellCommit"] == pinned[:12]
        assert json.loads((release_home / "Library/Application Support/HerdrShell/staged.json").read_text())["commit"] == pinned[:12]
        print("PASS release: moving REF builds pinned SHA and first-installs it")
        git("update-ref", "refs/heads/release-test", moved)
        result = subprocess.run(["bash", str(scripts / "release.sh"), "release-test"],
                                env={**release_env, "BAD_BUNDLE": old}, capture_output=True,
                                text=True, timeout=60)
        assert result.returncode != 0, result.stdout
        assert "bundle commit mismatch" in result.stderr, result.stderr
        assert json.loads((release_home / "Library/Application Support/HerdrShell/staged.json").read_text())["commit"] == pinned[:12]
        print("PASS release: mismatched build rejected before staging")
        targets.write_text(json.dumps({"targets": []}))
        # The publisher build path must allow an empty Studio and treat an identical
        # installed SHA as success, without reaching the build boundary again.
        shutil.rmtree(release_home / "Applications/Herdr Shell.app")
        for label in ("no installed app", "already installed"):
            result = subprocess.run([sys.executable, str(publisher), pinned],
                                    env=release_env, capture_output=True, text=True, timeout=60)
            assert result.returncode == 0, result.stdout + result.stderr
            with built_plist.open("rb") as f:
                assert plistlib.load(f)["HerdrShellCommit"] == pinned[:12]
            if label == "already installed":
                assert "already installed" in result.stdout, result.stdout
            print(f"PASS publish: {label} exits successfully")
print("PASS publisher no-downgrade and bundle identity CLI regression")
