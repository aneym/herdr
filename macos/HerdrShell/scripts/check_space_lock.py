#!/usr/bin/env python3
"""space.py holds the one-run Space lock; a second seat waits instead of replacing the app.

  python3 scripts/check_space_lock.py [--script path/to/space.py]

Incident 2026-10-06 11:15 ET: five seats shared the herdr-qa Space, and one seat's
`start --live` restarted the app and killed another seat's check mid-run. This runs the
real script with HOME in a temp dir and PATH=/usr/bin:/bin, which holds none of `cua`,
`lume-serve-ext` or `ssh`: any backend call fails at once with a FileNotFoundError naming
the tool, so the check sees every attempt and never reaches the real Space or Lume.
The holder is a hand-made lock in the manual format ("<name> <epoch>", no pid file).
"""
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

SCRIPT = pathlib.Path(__file__).resolve().parent / "space.py"
if "--script" in sys.argv:
    SCRIPT = pathlib.Path(sys.argv[sys.argv.index("--script") + 1]).resolve()
BACKEND = ("cua", "lume-serve-ext", "ssh")
failures = []


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def sandbox(holder_age_s):
    home = pathlib.Path(tempfile.mkdtemp(prefix="hsl-", dir="/tmp"))
    lock = home / ".agent-rails/locks/herdr-qa-space"
    if holder_age_s is not None:
        lock.mkdir(parents=True)
        (lock / "owner").write_text(f"seat-a {int(time.time()) - holder_age_s}\n")
    state = home / ".cua/herdr-space"
    state.mkdir(parents=True)
    (state / "app.json").write_text('{"exe": "/Users/lume/.herdr-space/bin/HerdrShell", "pid": "4242"}')
    return home, lock


def run(home, *args):
    env = {"HOME": str(home), "PATH": "/usr/bin:/bin", "HERDR_SPACE_OWNER": "seat-b"}
    t0 = time.time()
    r = subprocess.run([sys.executable, str(SCRIPT), *args], env=env, capture_output=True, text=True,
                       stdin=subprocess.DEVNULL, timeout=60)
    return r, time.time() - t0


def backend(r):
    """Backend tools the run tried to call."""
    return [t for t in BACKEND if f"No such file or directory: '{t}'" in r.stderr]


START = ("start", "--app", "/tmp/hsl-none/HerdrShell", "--socket", "/tmp/hsl-none/herdr.sock")

# 1. A live holder: start waits out --wait, exits 75 and never touches the backend.
home, lock = sandbox(holder_age_s=5)
before = (lock / "owner").read_text()
r, wall = run(home, *START, "--wait", "2")
check("held: start exits 75", r.returncode == 75, f"rc={r.returncode} stderr={r.stderr.strip()[-300:]}")
check("held: start says whom it waits for", "waiting for seat-a" in r.stderr, r.stderr.strip()[-200:])
check("held: start waits out --wait", wall >= 2, f"{wall:.1f}s")
check("held: no backend call (app not restarted)", backend(r) == [], str(backend(r)))
check("held: holder's lock untouched", (lock / "owner").read_text() == before)
shutil.rmtree(home)

# 2. A live holder: stop by another owner refuses with 75; --if-mine is a quiet no-op.
for args, rc in ((("stop",), 75), (("down",), 75), (("stop", "--force"), 75), (("stop", "--if-mine"), 0)):
    home, lock = sandbox(holder_age_s=5)
    r, _ = run(home, *args)
    label = " ".join(args)
    check(f"held: {label} exits {rc}", r.returncode == rc, f"rc={r.returncode} stderr={r.stderr.strip()[-200:]}")
    check(f"held: {label} stops nothing", backend(r) == [] and (lock / "owner").exists(), str(backend(r)))
    shutil.rmtree(home)

# 3. A stale holder (16 minutes, no owner process): start breaks it and reaches the
#    backend (which fails here), then drops the lock it took. Shows case 1 is not vacuous.
home, lock = sandbox(holder_age_s=16 * 60)
r, _ = run(home, *START, "--wait", "2")
check("stale: start breaks the lock", "broke a stale lock held by seat-a" in r.stderr, r.stderr.strip()[-200:])
check("stale: start reaches the backend", backend(r) == ["lume-serve-ext"], str(backend(r)))
check("stale: a failed start releases its lock", not lock.exists())
shutil.rmtree(home)

print(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
sys.exit(1 if failures else 0)
