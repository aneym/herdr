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


def run(home, *args, code=None):
    env = {"HOME": str(home), "PATH": "/usr/bin:/bin", "HERDR_SPACE_OWNER": "seat-b",
           "PYTHONDONTWRITEBYTECODE": "1"}
    t0 = time.time()
    argv = [sys.executable, "-c", code, str(SCRIPT)] if code is not None else [sys.executable, str(SCRIPT), *args]
    r = subprocess.run(argv, env=env, capture_output=True, text=True,
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

# 3. A stale holder (16 minutes, recorded dead owner process): start breaks it and reaches the
#    backend (which fails here), then drops the lock it took. Shows case 1 is not vacuous.
home, lock = sandbox(holder_age_s=16 * 60)
dead = subprocess.Popen([sys.executable, "-c", "pass"], stdin=subprocess.DEVNULL)
dead.wait()
(lock / "pid").write_text(str(dead.pid))
r, _ = run(home, *START, "--wait", "2")
check("stale: start breaks the lock", "broke a stale lock held by seat-a" in r.stderr, r.stderr.strip()[-200:])
check("stale: start reaches the backend", backend(r) == ["lume-serve-ext"], str(backend(r)))
check("stale: a failed start releases its lock", not lock.exists())
shutil.rmtree(home)

# 4. An aged manual lock has unknown process liveness: start cannot break it, but
#    explicit --force can. These CLI checks exercise the real filesystem boundary.
home, lock = sandbox(holder_age_s=16 * 60)
before = (lock / "owner").read_text()
r, _ = run(home, *START, "--wait", "0")
check("manual: aged lock makes start exit 75", r.returncode == 75, f"rc={r.returncode} stderr={r.stderr.strip()[-300:]}")
check("manual: start leaves the lock untouched", (lock / "owner").exists() and (lock / "owner").read_text() == before)
check("manual: start calls no backend", backend(r) == [], str(backend(r)))
shutil.rmtree(home)
home, lock = sandbox(holder_age_s=16 * 60)
r, _ = run(home, "stop", "--force")
check("manual: force breaks the aged lock", backend(r) == ["cua"] and not lock.exists(), f"rc={r.returncode} backend={backend(r)}")
shutil.rmtree(home)

# 5. A breaker with an obsolete observation must not move a new holder's directory,
#    even transiently. An audit hook observes real filesystem renames without mocking
#    lock code; a final-state assertion alone misses the old rename-back race.
home, lock = sandbox(holder_age_s=0)
r, _ = run(home, code='''
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location("space", sys.argv[1])
space = importlib.util.module_from_spec(spec)
spec.loader.exec_module(space)
lock = pathlib.Path(space.LOCK)
before = (lock / "owner").read_text()
renames = []
sys.addaudithook(lambda event, args: renames.append(args) if event == "os.rename" else None)
assert space.lock_break({"owner": "old-seat", "epoch": 1, "pid": 0}) is False
assert (lock / "owner").read_text() == before
assert not renames, f"fresh lock was moved: {renames}"
''')
check("breaker: stale observation leaves fresh lock unmoved", r.returncode == 0, r.stderr.strip()[-300:])
shutil.rmtree(home)

print(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
sys.exit(1 if failures else 0)
