#!/usr/bin/env python3
"""P25 check: prod and dev bundles, headless channel and update selftests.

No window. The update selftest uses a directory under ~/.cache, never ~/Applications.
"""
import json
import os
import plistlib
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

    def load_json(path):
        if not os.path.isfile(path):
            return {}
        try:
            return json.loads(open(path).read())
        except json.JSONDecodeError:
            return {}

    roll = load_json(os.path.join(update_root, "corrupt-proof.json"))
    check(
        "rollback",
        roll.get("restored") == "good"
        and "corrupt" in roll.get("log", "")
        and roll.get("log", "").startswith("bad ")
        and roll.get("retry") is False
        and roll.get("pill") == "hidden"
        and roll.get("staged_remains") is True
        and roll.get("newer_pill") == "update"
        and roll.get("apply") is False,
        roll.get("log", ""),
    )
    retry = load_json(os.path.join(update_root, "retry-proof.json"))
    check(
        "retry",
        retry.get("first_ok") is False
        and retry.get("restored") == "kept"
        and retry.get("staged") == "next"
        and retry.get("retry") is True
        and retry.get("pill") == "retry"
        and retry.get("second_ok") is True
        and retry.get("installed") == "next"
        and retry.get("staged_gone") is True,
        str(retry.get("pill")),
    )
    restart_doc = load_json(os.path.join(update_root, "support", "restart.json"))
    swap_s = restart_doc.get("restart_swap_s", -1)
    check("restart_swap_s", isinstance(swap_s, (int, float)) and 0 <= swap_s < 2, f"{swap_s}s")

    hits = []
    src_root = os.path.join(D0, "Sources")
    for dirpath, _, files in os.walk(src_root):
        for name in files:
            if not name.endswith(".swift"):
                continue
            path = os.path.join(dirpath, name)
            update_file = "update" in name.lower()
            with open(path) as handle:
                for number, line in enumerate(handle, 1):
                    folded = line.lower()
                    rel = os.path.relpath(path, src_root)
                    if "pkill" in folded or "killall" in folded:
                        hits.append(f"{rel}:{number}:kill")
                    if not update_file:
                        continue
                    if "kill(" in folded or ".sock" in folded:
                        hits.append(f"{rel}:{number}:update-path")
                    if "herdr" in folded and "herdr shell.app" not in folded:
                        hits.append(f"{rel}:{number}:herdr")
    check("update path", not hits, " ".join(hits) if hits else "case-insensitive scan of Sources")

    argv = load_json(os.path.join(update_root, "open-argv.json"))
    expected = ""
    expected_file = os.path.join(update_root, "open-expected.txt")
    if os.path.isfile(expected_file):
        expected = open(expected_file).read().strip()
    real_expected = os.path.realpath(expected) if expected else ""
    scratch_real = os.path.realpath(update_root)
    check(
        "helper opens the app",
        argv == ["/usr/bin/open", expected]
        and "-g" not in argv
        and "-j" not in argv
        and real_expected.startswith(scratch_real + os.sep)
        and real_expected != REAL_APPS
        and not real_expected.startswith(REAL_APPS + os.sep),
        " ".join(argv) if isinstance(argv, list) else str(argv),
    )

    say(f"restart_swap_s {swap_s}")
    if failures:
        say(f"{len(failures)} failed")
        return 1
    say("ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
