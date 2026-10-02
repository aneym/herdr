#!/usr/bin/env python3
"""P25 check: prod and dev bundles, headless channel and update selftests.

No window. The update selftest uses a directory under ~/.cache, never ~/Applications.
"""
import json
import os
import plistlib
import re
import subprocess
import sys

D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ROOT = subprocess.check_output(["git", "-C", D0, "rev-parse", "--show-toplevel"], text=True).strip()
SCRATCH = os.path.expanduser("~/.cache/herdr-build/p25")
REAL_APPS = os.path.realpath(os.path.expanduser("~/Applications"))
BIN = os.path.join(D0, ".build", "release", "HerdrShell")
lines, failures = [], []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def run(argv, env=None):
    e = os.environ.copy()
    e.pop("SHELL_LAB", None)
    if env:
        e.update(env)
    return subprocess.run(argv, capture_output=True, text=True, env=e)


def plist(app):
    with open(os.path.join(app, "Contents", "Info.plist"), "rb") as f:
        return plistlib.load(f)


def main():
    if os.path.realpath(SCRATCH).startswith(REAL_APPS + os.sep) or os.path.realpath(SCRATCH) == REAL_APPS:
        check("scratch is not ~/Applications", False, SCRATCH)
        return 1
    os.makedirs(SCRATCH, exist_ok=True)
    short = subprocess.check_output(["git", "-C", ROOT, "rev-parse", "--short=12", "HEAD"], text=True).strip()

    build = subprocess.run(["nice", "-n", "10", "swift", "build", "-c", "release"], cwd=D0, capture_output=True, text=True)
    check("swift build", build.returncode == 0, "clean" if build.returncode == 0 else build.stderr[-400:])
    if build.returncode != 0:
        return 1

    prod_app = subprocess.check_output(["bash", os.path.join(D0, "scripts", "bundle.sh"), "prod", SCRATCH], text=True).strip()
    dev_app = subprocess.check_output(["bash", os.path.join(D0, "scripts", "bundle.sh"), "dev", SCRATCH], text=True).strip()
    prod_bin = os.path.join(prod_app, "Contents", "MacOS", "HerdrShell")
    dev_bin = os.path.join(dev_app, "Contents", "MacOS", "HerdrShell")

    for app, bid, name, label in (
        (prod_app, "com.aneyman.herdr-shell", "Herdr Shell", "prod"),
        (dev_app, "com.aneyman.herdr-shell.dev", "Herdr Shell Dev", "dev"),
    ):
        info = plist(app)
        sign = subprocess.run(["codesign", "--verify", "--verbose=2", app], capture_output=True, text=True)
        check(f"{label} bundle id", info.get("CFBundleIdentifier") == bid, info.get("CFBundleIdentifier", ""))
        check(f"{label} name", info.get("CFBundleName") == name, info.get("CFBundleName", ""))
        check(f"{label} commit", info.get("HerdrShellCommit") == short, info.get("HerdrShellCommit", ""))
        check(f"{label} built at", bool(info.get("HerdrShellBuiltAt")), info.get("HerdrShellBuiltAt", ""))
        check(f"{label} min system", info.get("LSMinimumSystemVersion") == "14.0")
        check(f"{label} hidpi", info.get("NSHighResolutionCapable") is True)
        check(f"{label} codesign", sign.returncode == 0, "valid" if sign.returncode == 0 else sign.stderr.strip())

    prod = json.loads(run([prod_bin, "--selftest-channel"]).stdout)
    dev = json.loads(run([dev_bin, "--selftest-channel"]).stdout)
    home = os.path.expanduser("~")
    check("prod channel", prod.get("channel") == "prod" and prod.get("bundle_id") == "com.aneyman.herdr-shell")
    check("prod defaults", prod.get("defaults_domain") == "com.aneyman.herdr-shell", prod.get("defaults_domain", ""))
    check(
        "prod support",
        prod.get("app_support") == os.path.join(home, "Library/Application Support/HerdrShell"),
        prod.get("app_support", ""),
    )
    check("dev channel", dev.get("channel") == "dev" and dev.get("bundle_id") == "com.aneyman.herdr-shell.dev")
    check("dev defaults", dev.get("defaults_domain") == "herdr.shell.dev.live", dev.get("defaults_domain", ""))
    check(
        "dev support",
        dev.get("app_support") == os.path.join(home, "Library/Application Support/HerdrShell Dev"),
        dev.get("app_support", ""),
    )

    refused = run([dev_bin, "--selftest-channel", "--allow-live"])
    check(
        "dev refuses allow-live",
        refused.returncode != 0 and "allow-live" in refused.stderr,
        f"exit {refused.returncode}",
    )
    fifo = os.path.join(SCRATCH, "no.fifo")
    refused_c = run([prod_bin, "--selftest-channel", "--control", fifo])
    check(
        "prod refuses control",
        refused_c.returncode != 0 and "control" in refused_c.stderr and not os.path.exists(fifo),
        f"exit {refused_c.returncode}",
    )

    update_root = os.path.join(SCRATCH, "update-root")
    if os.path.realpath(update_root).startswith(REAL_APPS):
        check("update root", False, update_root)
        return 1
    upd = run([prod_bin, "--selftest-update", update_root])
    try:
        report = json.loads(upd.stdout)
    except json.JSONDecodeError:
        report = {}
    check("selftest exit", upd.returncode == 0, f"exit {upd.returncode} {upd.stderr[-300:]}")
    check("same commit", report.get("same_commit", {}).get("update") is False)
    diff = report.get("different_commit", {})
    check(
        "different commit",
        diff.get("update") is True and diff.get("notes") == ["ship the shell", "stage the update"],
        str(diff.get("notes")),
    )
    proof_path = os.path.join(update_root, "swap-proof.txt")
    proof = open(proof_path).read() if os.path.isfile(proof_path) else ""
    check("swap", "installed=new" in proof and "previous=old" in proof, proof.replace("\n", " "))
    restored = ""
    marker = os.path.join(update_root, "Applications", "Herdr Shell.app", "Contents", "Resources", "marker")
    if os.path.isfile(marker):
        restored = open(marker).read().strip()
    log_path = os.path.join(update_root, "support", "update.log")
    log_text = open(log_path).read() if os.path.isfile(log_path) else ""
    check("rollback", restored == "good" and "corrupt" in log_text, f"marker {restored}")
    restart_s = report.get("restart_s", -1)
    check("restart", isinstance(restart_s, (int, float)) and 0 <= restart_s < 2, f"{restart_s}s")

    pat = re.compile(r"herdr[^\n]{0,80}\b(stop|kill|restart|server)\b", re.I)
    blob_paths = [
        "Sources/HerdrShell/UpdateRestart.swift",
        "Sources/HerdrShell/UpdatePill.swift",
        "Sources/HerdrShell/Channel.swift",
        "scripts/release.sh",
        "scripts/bundle.sh",
        "scripts/dev.sh",
    ]
    bad = []
    for rel in blob_paths:
        text = open(os.path.join(D0, rel)).read()
        if pat.search(text):
            bad.append(rel)
    diff_text = subprocess.run(
        ["git", "-C", ROOT, "diff", "ddbd3a52", "--",
         "macos/HerdrShell/Sources/HerdrShell/UpdateRestart.swift",
         "macos/HerdrShell/Sources/HerdrShell/UpdatePill.swift",
         "macos/HerdrShell/Sources/HerdrShell/Channel.swift",
         "macos/HerdrShell/Sources/HerdrShell/main.swift",
         "macos/HerdrShell/scripts/release.sh",
         "macos/HerdrShell/scripts/bundle.sh",
         "macos/HerdrShell/scripts/dev.sh"],
        capture_output=True, text=True,
    ).stdout
    if pat.search(diff_text):
        bad.append("diff")
    check("update path", not bad, " ".join(bad) if bad else "no herdr stop kill restart server")
    open_path = "open" in open(os.path.join(D0, "Sources/HerdrShell/UpdateRestart.swift")).read()
    check("helper opens the app", "/usr/bin/open" in open(os.path.join(D0, "Sources/HerdrShell/UpdateRestart.swift")).read() and open_path)

    say(f"restart_s {restart_s}")
    if failures:
        say(f"{len(failures)} failed")
        return 1
    say("ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
