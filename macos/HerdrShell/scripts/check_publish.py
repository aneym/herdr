#!/usr/bin/env python3
"""CLI regression: cached releases cannot overwrite newer or unknown installs.

Uses real git histories, plist probing and the publisher CLI. No existing test
covers delivery ancestry; a stale-cache regression must skip before staging.
No production seams or standalone executable stubs are needed.
"""
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

publisher = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("publish.py")
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
    for installed, reason in ((new, "not a strict descendant"), ("a" * 40, "unknown installed/release commit")):
        with plist.open("wb") as f:
            plistlib.dump({"HerdrShellCommit": installed}, f)
        for command in (("fanout",), ("install", "test")):
            result = subprocess.run([sys.executable, str(publisher), *command], env=env,
                                    capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, result.stderr
            assert reason in result.stdout, result.stdout
            assert "FAIL deliver" not in result.stdout, result.stdout
            assert json.loads(sentinel.read_text())["commit"] == new
            with plist.open("rb") as f:
                assert plistlib.load(f)["HerdrShellCommit"] == installed
            print(f"PASS {' '.join(command)}: {reason}; installed/staged unchanged")
print("PASS publisher no-downgrade CLI regression")
