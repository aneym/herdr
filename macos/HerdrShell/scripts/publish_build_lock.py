import os
import shutil
import time
from contextlib import contextmanager

HOME = os.path.expanduser("~")

BUILD_LOCK = os.environ.get(
    "HERDR_STUDIO_BUILD_LOCK", f"{HOME}/.agent-rails/locks/studio-build"
)


BUILD_LOCK_STALE = 30 * 60


def build_lock_age():
    """Seconds since the lock's owner file (or, before it is written, the lock dir) changed;
    None once the lock is gone."""
    for path in (f"{BUILD_LOCK}/owner", BUILD_LOCK):
        try:
            return time.time() - os.stat(path).st_mtime
        except FileNotFoundError:
            continue
    return None


def break_stale_build_lock():
    """Breakers take a mutex and re-check the age inside it, so a lock another builder took
    after this one saw the stale lock is never removed."""
    mutex = f"{BUILD_LOCK}.break"
    try:
        os.mkdir(mutex)
    except OSError:
        return False
    try:
        age = build_lock_age()
        if age is None or age <= BUILD_LOCK_STALE:
            return False
        aside = f"{BUILD_LOCK}.stale-{os.getpid()}"
        try:
            os.rename(BUILD_LOCK, aside)
        except OSError:
            return False
        shutil.rmtree(aside, ignore_errors=True)
        return True
    finally:
        os.rmdir(mutex)


@contextmanager
def studio_build_lock(log):
    """Studio runs one build at a time (Rails orchestrator rule, 2026-10-06): the shared
    mkdir lock with an owner file "<name> <epoch>", stale after 30 minutes."""
    os.makedirs(os.path.dirname(BUILD_LOCK), exist_ok=True)
    announced = False
    while True:
        try:
            os.mkdir(BUILD_LOCK)
            break
        except FileExistsError:
            age = build_lock_age()
            if age is None:
                continue
            if age > BUILD_LOCK_STALE and break_stale_build_lock():
                continue
            if not announced:
                log("waiting for the studio-build lock")
                announced = True
            time.sleep(20)
    try:
        with open(f"{BUILD_LOCK}/owner", "w") as f:
            f.write(f"herdr-shell-publish {int(time.time())}\n")
        yield
    finally:
        shutil.rmtree(BUILD_LOCK, ignore_errors=True)
