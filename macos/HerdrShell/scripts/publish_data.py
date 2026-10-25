import hashlib
import json
import os
import shutil
import subprocess
import tempfile

from publish_transport import on

HOME = os.path.expanduser("~")

DATA_DIR = f"{HOME}/.agent-rails/herdr"


DATA_FILES = ("lanes.json", "areas.json", "modes.json", "overlay.json")


REMOTE_DATA = r"""
set -e
D="$HOME/.agent-rails/herdr"
mkdir -p "$D"
IN=$(mktemp -d "$D/.incoming.XXXXXX")
tar -xf - -C "$IN"
for f in "$IN"/*.json; do mv "$f" "$D/$(basename "$f")"; done
rmdir "$IN"
echo synced
"""


def data(logdir, targets, log):
    """Push the Areas/Parked inputs to each target when their content changed."""
    have = [f for f in DATA_FILES if os.path.isfile(f"{DATA_DIR}/{f}")]
    if not have:
        return

    with tempfile.TemporaryDirectory(prefix="herdr-data-", dir=logdir) as snapshot:
        digests = {}
        for f in have:
            shutil.copyfile(f"{DATA_DIR}/{f}", f"{snapshot}/{f}")
            with open(f"{snapshot}/{f}", "rb") as fh:
                digests[f] = hashlib.sha256(fh.read()).hexdigest()
        send_data(snapshot, digests, logdir, targets, log)


def send_data(snapshot, digests, logdir, targets, log):
    state_path = f"{logdir}/data-state.json"
    try:
        with open(state_path) as fh:
            state = json.load(fh)
    except (OSError, ValueError):
        state = {}
    for t in targets:
        if t.get("local"):
            continue
        name = t.get("name", "?")
        sent = state.get(name)
        if not isinstance(sent, dict):
            sent = {}
        changed = [f for f, digest in digests.items() if sent.get(f) != digest]
        if not changed:
            continue
        tar = subprocess.Popen(
            ["tar", "-cf", "-", "-C", snapshot, *changed], stdout=subprocess.PIPE
        )
        try:
            r = subprocess.run(
                on(t, REMOTE_DATA),
                stdin=tar.stdout,
                check=False,
                capture_output=True,
                text=True,
                timeout=60,
            )
        except (OSError, subprocess.TimeoutExpired):
            r = None
        tar.stdout.close()
        tar_code = tar.wait()
        if (
            r is not None
            and r.returncode == 0
            and tar_code == 0
            and "synced" in r.stdout
        ):
            state[name] = {**sent, **{f: digests[f] for f in changed}}
            state.pop(f"{name}.down", None)
        elif not state.get(f"{name}.down"):
            state[f"{name}.down"] = True
            log(f"{name}: data sync failed, will retry quietly")
    os.makedirs(logdir, exist_ok=True)
    with open(state_path + ".tmp", "w") as fh:
        json.dump(state, fh)
    os.replace(state_path + ".tmp", state_path)
