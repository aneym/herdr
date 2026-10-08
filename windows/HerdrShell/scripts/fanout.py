#!/usr/bin/env python3
"""One Studio-side, game-guarded fetch/stage pass; never installs the shell."""

import fcntl
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import time
from datetime import datetime, timezone

import pc

CHECKOUT = Path('/Volumes/StudioExt/repos/herdr-wt/winshell-fanout')
MAIN = Path('/Volumes/StudioExt/repos/herdr')
STATE = Path.home() / '.local/state/herdr-winshell-fanout/state.json'
LOG = Path.home() / 'Library/Logs/herdr-winshell-fanout.log'
LOCK_STALE_S = 3600


def decide(state, newest_sha, game_running, lock_age_s):
    """Choose a pass action; a fresh competing lock takes precedence."""
    if lock_age_s is not None and lock_age_s < LOCK_STALE_S:
        return 'wait'
    if state.get('built') == newest_sha:
        return 'skip'
    if game_running:
        return 'wait'
    return 'build'


# A sha whose off-PC build or fetch failed is retried after 30, 60 min, then left
# alone until main moves (pc.py also refuses to dispatch a sha that failed twice).
FETCH_TRIES = 3
FETCH_BACKOFF_S = 1800
FETCH_TIMEOUT_S = 3900


def fetch_due(state, sha, now):
    """Whether a fetch of sha may run now, given state['fetch_failures']."""
    failure = state.get('fetch_failures', {}).get(sha)
    if not failure:
        return True
    if failure['n'] >= FETCH_TRIES:
        return False
    return now - failure['last'] >= FETCH_BACKOFF_S * 2 ** (failure['n'] - 1)


def timestamp():
    return datetime.now(timezone.utc).isoformat()


def log(event):
    LOG.parent.mkdir(parents=True, exist_ok=True)
    with LOG.open('a') as out:
        out.write(f'{timestamp()} {event}\n')


def read_state(path=None):
    path = path or STATE
    try:
        with path.open() as source:
            state = json.load(source)
    except FileNotFoundError:
        return {'built': None, 'staged_at': None, 'last_error': None}
    if not isinstance(state, dict):
        raise ValueError('state must be a JSON object')
    return state


def write_state(state, path=None):
    path = path or STATE
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix('.json.tmp')
    with temporary.open('w') as out:
        json.dump(state, out)
        out.write('\n')
        out.flush()
        os.fsync(out.fileno())
    os.replace(temporary, path)


def git(*args, repo=CHECKOUT):
    result = subprocess.run(['git', '-C', str(repo), *args],
                            capture_output=True, text=True)
    if result.returncode:
        # Do not persist command output: remotes can contain credentials.
        raise RuntimeError(f'git {args[0]} failed ({result.returncode})')
    return result.stdout.strip()


def run_pass(state):
    if not CHECKOUT.exists():
        git('worktree', 'add', '--detach', str(CHECKOUT), 'origin/main', repo=MAIN)
    git('fetch', 'origin', 'main')
    if git('status', '--porcelain'):
        raise RuntimeError('dedicated checkout is dirty; refusing to overwrite it')
    git('checkout', '--detach', 'origin/main')
    sha = git('log', '-1', '--format=%H', 'origin/main', '--', 'windows/HerdrShell')
    if not sha:
        raise RuntimeError('no Windows shell commit found')
    if decide(state, sha, False, None) == 'skip':
        log(f'unchanged {sha}')
        return 0
    # Build the shell-touching commit, not an unrelated newer main commit.
    git('checkout', '--detach', sha)
    # App Control on the PC blocks local cargo builds; a GitHub Windows runner
    # builds it and the PC only receives the files. Copying files is safe
    # during a game, so the fetch never waits on the guard.
    if state.get('fetched') != sha:
        if not fetch_due(state, sha, time.time()):
            log(f'fetch of {sha} backing off after failures')
            return 0
        log(f'building {sha}')
        try:
            result = subprocess.run([sys.executable, str(pc.HERE / 'pc.py'),
                                     'fetch', '--sha', sha, '--dispatch'],
                                    capture_output=True, text=True, timeout=FETCH_TIMEOUT_S)
            code, stdout = result.returncode, result.stdout or ''
        except subprocess.TimeoutExpired as error:
            # A hung build or copy backs off like a failed one.
            code, stdout = f'timed out after {error.timeout} s', ''
            if isinstance(error.stdout, bytes):
                stdout = error.stdout.decode(errors='replace')
            elif error.stdout:
                stdout = error.stdout
        run = re.search(r'build run (\d+)|\(run (\d+)\)', stdout)
        if run:
            state.setdefault('runs', {})[sha] = run.group(1) or run.group(2)
        if code:
            failures = state.setdefault('fetch_failures', {})
            n = failures.get(sha, {}).get('n', 0) + 1
            failures[sha] = {'n': n, 'last': time.time()}
            write_state(state)
            raise RuntimeError(f'off-PC build or fetch failed ({code}), try {n}')
        state.get('fetch_failures', {}).pop(sha, None)
        state.update(fetched=sha)
        write_state(state)
    # Only game_guard.ps1 here: it runs in the ssh session and never touches
    # Alex's desktop. Staging is a gated helper and waits for the game to end.
    game, _ = pc.guard(quiet=True)
    if decide(state, sha, game, None) == 'wait':
        log(f'fetched {sha}; game running, staging next tick')
        return 0
    rc = pc.scp_to(pc.HERE / 'pc/stage.ps1', f'{pc.R_SCRIPTS}/stage.ps1')
    if rc:
        raise RuntimeError(f'stage helper upload failed ({rc})')
    rc, _ = pc.ps_file('stage.ps1', '-Sha', sha)
    if rc:
        raise RuntimeError(f'PC staging failed ({rc})')
    state.update(built=sha, staged_at=timestamp(), last_error=None)
    write_state(state)
    log(f'staged {sha}')
    return 0


def main():
    STATE.parent.mkdir(parents=True, exist_ok=True)
    lock_path = STATE.parent / 'pass.lock'
    # A kernel lock prevents overlaps even when a live build exceeds 60 minutes.
    # Dead owners release it automatically; old on-disk locks are reusable.
    with lock_path.open('a+') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            log('another pass owns the lock; retry later')
            return 75
        lock.seek(0)
        lock.truncate()
        lock.write(str(os.getpid()))
        lock.flush()
        os.utime(lock_path, None)
        state = {'built': None, 'staged_at': None, 'last_error': None}
        try:
            state = read_state()
            return run_pass(state)
        except pc.Gated:
            log('deferred by game guard')
            return 75
        except subprocess.TimeoutExpired as error:
            message = f'PC call timed out after {error.timeout} seconds'
            state['last_error'] = message
            write_state(state)
            log(f'failure: {message}')
            return 75
        except Exception as error:
            # Persist only controlled error types/messages, never subprocess output.
            message = str(error) if isinstance(error, RuntimeError) else type(error).__name__
            state['last_error'] = message
            write_state(state)
            log(f'failure: {message}')
            return 1


if __name__ == '__main__':
    sys.exit(main())
