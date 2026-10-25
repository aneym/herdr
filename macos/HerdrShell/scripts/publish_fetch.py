import fcntl
import hashlib
import json
import os
import subprocess
import time
from pathlib import Path


def shared_fetch(repo, branch, logdir, max_age=60):
    common = subprocess.run(
        ['git', '-C', repo, 'rev-parse', '--path-format=absolute', '--git-common-dir'],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    key = hashlib.sha256(f'{os.path.realpath(common)}:{branch}'.encode()).hexdigest()[:20]
    directory = Path(logdir)
    directory.mkdir(parents=True, exist_ok=True)
    state_path = directory / f'fetch-{key}.json'
    ref = f'refs/remotes/origin/{branch}'
    with (directory / f'fetch-{key}.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | (fcntl.LOCK_NB if max_age else 0))
        except BlockingIOError:
            return False
        tip = subprocess.run(['git', '-C', repo, 'rev-parse', '--verify', ref], capture_output=True, text=True).stdout.strip()
        try:
            state = json.loads(state_path.read_text())
        except (OSError, ValueError):
            state = {}
        age = time.time() - state.get('finished_at', 0)
        if tip and tip == state.get('sha') and 0 <= age < (max_age if state.get('ok') else min(max_age, 30)):
            return False
        started = time.monotonic()
        ok = False
        try:
            result = subprocess.run(
                ['git', '-C', repo, 'fetch', '-q', '--no-tags', '--recurse-submodules=no', 'origin', f'+refs/heads/{branch}:{ref}'],
                capture_output=True, timeout=60, env={**os.environ, 'HERDR_SHELL_PUBLISHING': '1'},
            )
            ok = result.returncode == 0
        except (OSError, subprocess.TimeoutExpired):
            pass
        tip = subprocess.run(['git', '-C', repo, 'rev-parse', '--verify', ref], capture_output=True, text=True).stdout.strip()
        state = {'sha': tip, 'ok': ok, 'finished_at': time.time(), 'duration_s': round(time.monotonic() - started, 3)}
        temporary = state_path.with_suffix(f'.{os.getpid()}.tmp')
        temporary.write_text(json.dumps(state))
        os.replace(temporary, state_path)
        print(f'fetch shared: ok={ok} duration={state["duration_s"]}s sha={tip[:12]}', flush=True)
        return ok
