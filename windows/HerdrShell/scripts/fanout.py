#!/usr/bin/env python3
"""One Studio-side, game-guarded build/stage pass; never installs the shell."""

import fcntl
import json
import os
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


def timestamp():
    return datetime.now(timezone.utc).isoformat()


def log(event):
    LOG.parent.mkdir(parents=True, exist_ok=True)
    with LOG.open('a') as out:
        out.write(f'{timestamp()} {event}\n')


def read_state(path=STATE):
    try:
        with path.open() as source:
            state = json.load(source)
    except FileNotFoundError:
        return {'built': None, 'staged_at': None, 'last_error': None}
    if not isinstance(state, dict):
        raise ValueError('state must be a JSON object')
    return state


def write_state(state, path=STATE):
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
    status = subprocess.run([sys.executable, str(pc.HERE / 'pc.py'), 'status'],
                            capture_output=True, text=True, timeout=120)
    if status.returncode:
        raise RuntimeError(f'PC status failed ({status.returncode})')
    game = json.loads(status.stdout)['game']['game']
    if not isinstance(game, bool):
        raise ValueError('PC game status is not boolean')
    if decide(state, sha, game, None) == 'wait':
        log('game running; retry next tick')
        return 0
    # Build the shell-touching commit, not an unrelated newer main commit.
    git('checkout', '--detach', sha)
    log(f'building {sha}')
    result = subprocess.run([sys.executable, str(pc.HERE / 'pc.py'),
                             'build', '--src', str(CHECKOUT)],
                            capture_output=True, text=True, timeout=3600)
    if result.returncode == 75:
        log('build deferred by game guard')
        return 75
    if result.returncode:
        raise RuntimeError(f'PC build failed ({result.returncode})')
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
