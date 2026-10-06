#!/usr/bin/env python3
"""Lab herdr session for HerdrShell (default name 'shellspike'; set SHELL_LAB to use
another, e.g. SHELL_LAB=shellspike-p2 so pieces do not share a lab).

  lab.py up           start the lab server (spike binary, isolated HOME/XDG) with
                      one workspace, a tab of two plain-shell panes, a lane tab
                      and a workflow tab
  lab.py env          print the lab env as KEY=VALUE lines (for the app launcher)
  lab.py herdr ARGS   run the spike herdr against the lab session
  lab.py down         stop ONLY the lab server

Safety: the environment is built from nothing (no HERDR_*, CLAUDE*), HOME and
XDG dirs live under LAB, and every socket path is asserted to be inside LAB.
The live session's socket, config and binary are never used.
"""
import hashlib
import json
import os
import subprocess
import sys
import time

NAME = os.environ.get("SHELL_LAB", "shellspike")
assert NAME.startswith("shellspike") and "/" not in NAME, "SHELL_LAB must be shellspike[-suffix]"


def _lab_dir(name):
    """Directory for this lab. A long name makes the herdr socket exceed sun_path
    (104 bytes, and herdr appends a suffix), so those labs keep the session name
    and use a shorter directory."""
    direct = os.path.expanduser(f"~/.cache/herdr-build/{name}")
    sock = os.path.join(direct, "h", ".config", "herdr", "sessions", name, "herdr.sock")
    if len(sock.encode()) <= 88:  # herdr appends a suffix (e.g. ".client.sock") to this path
        return direct
    # A hash, not the name's tail: tails collide (shellspike-p, shellspike-long-p), and the
    # path must stay short enough for ssh's socket forward of herdr-client.sock too.
    tail = hashlib.sha1(name.encode()).hexdigest()[:6]
    return os.path.expanduser(f"~/.cache/herdr-build/s/{tail}")


LAB = _lab_dir(NAME)
# Until P1 (--no-escape) ships in the herdr on PATH, the lab uses the spike binary.
BIN_SRC = os.environ.get("HERDR_SHELL_BIN") or os.path.expanduser("~/.cache/herdr-build/target-pane-attach/release/herdr")
BIN = os.path.join(LAB, "bin", "herdr")
SESSION = NAME
LIVE_BIN = os.path.expanduser("~/.local/bin/herdr")
HOME = os.path.join(LAB, "h")  # short: the socket path must fit sun_path (104 bytes, herdr adds a suffix)
CFG = os.path.join(HOME, ".config")
SOCK = os.path.join(CFG, "herdr", "sessions", SESSION, "herdr.sock")


def env():
    e = {
        "HOME": HOME,
        "USER": os.environ.get("USER", "lab"),
        "LOGNAME": os.environ.get("USER", "lab"),
        "SHELL": "/bin/zsh",
        "TERM": "xterm-256color",
        "LANG": "en_US.UTF-8",
        "PATH": f"{os.path.dirname(BIN)}:/usr/bin:/bin:/usr/sbin:/sbin",
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
        "XDG_CONFIG_HOME": CFG,
        "XDG_STATE_HOME": os.path.join(HOME, ".local", "state"),
        "XDG_CACHE_HOME": os.path.join(HOME, ".cache"),
        "XDG_DATA_HOME": os.path.join(HOME, ".local", "share"),
        "HERDR_SOCKET_PATH": SOCK,
    }
    for k, v in e.items():
        if k.startswith(("HERDR_SOCKET", "XDG_", "HOME")) and not v.startswith(LAB + os.sep):
            sys.exit(f"refusing: {k} outside lab")
    assert os.path.realpath(BIN) != os.path.realpath(LIVE_BIN)
    return e


def herdr(*args, check=True, capture=True):
    cmd = ["nice", "-n", "10", BIN, "--session", SESSION, *args]
    r = subprocess.run(cmd, env=env(), capture_output=capture, text=True)
    if check and r.returncode != 0:
        sys.exit(f"lab herdr {' '.join(args)} failed: {r.stderr.strip()}")
    return r.stdout


def spawn_detached(cmd, log):
    pid = os.fork()
    if pid == 0:
        os.setsid()
        if os.fork() == 0:
            fd = os.open(log, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o644)
            nul = os.open(os.devnull, os.O_RDONLY)
            os.dup2(nul, 0); os.dup2(fd, 1); os.dup2(fd, 2)
            os.chdir(HOME)
            os.execve(cmd[0], cmd, env())
        os._exit(0)
    os.waitpid(pid, 0)


def up():
    # Fresh session every time: drop the lab's persisted state (only under LAB).
    import shutil
    assert NAME.startswith("shellspike") and HOME.startswith(LAB + os.sep)
    # A short hashed dir can be shared by two names; never wipe another lab's home.
    owner = os.path.join(LAB, ".lab-name")
    try:
        with open(owner) as f:
            other = f.read().strip()
    except OSError:
        other = NAME
    if other != NAME:
        sys.exit(f"lab dir {LAB} belongs to {other}; pick another SHELL_LAB")
    os.makedirs(LAB, exist_ok=True)
    with open(owner, "w") as f:
        f.write(NAME)
    if not (os.path.exists(SOCK) and subprocess.run(
            [BIN, "--session", SESSION, "workspace", "list"], env=env(), capture_output=True).returncode == 0):
        shutil.rmtree(HOME, ignore_errors=True)
    for d in (os.path.dirname(BIN), os.path.join(CFG, "herdr"), os.path.join(HOME, ".local", "state"),
              os.path.join(HOME, ".cache"), os.path.join(HOME, ".local", "share")):
        os.makedirs(d, exist_ok=True)
    # New inode each time: cp over a Mach-O in place gets it SIGKILLed on macOS
    # (stale code-signature cache for the old vnode).
    subprocess.run(["cp", BIN_SRC, BIN + ".new"], check=True)
    os.replace(BIN + ".new", BIN)
    with open(os.path.join(CFG, "herdr", "config.toml"), "w") as f:
        f.write("[session]\nresume_agents_on_restore = false\n")
    with open(os.path.join(CFG, "herdr", "plugins.json"), "w") as f:
        f.write("[]\n")
    with open(os.path.join(HOME, ".zshrc"), "w") as f:
        f.write("PS1='%1~ %# '\n")
    open(os.path.join(HOME, ".zshenv"), "w").close()
    if os.path.exists(SOCK) and herdr("workspace", "list", check=False).strip():
        print("lab already up")
        return
    spawn_detached(["/usr/bin/nice", "-n", "10", BIN, "--session", SESSION, "server"], os.path.join(LAB, "server.log"))
    for _ in range(100):
        if os.path.exists(SOCK) and subprocess.run(
                [BIN, "--session", SESSION, "workspace", "list"], env=env(), capture_output=True).returncode == 0:
            break
        time.sleep(0.1)
    else:
        sys.exit("lab server did not come up; see server.log")
    print(herdr("workspace", "list"))


def seed():
    """Idempotent lab layout: orchestrator tab, a lane tab with two plain shells
    (the scenario panes), a lane that owns a PC workflow, and a workflow owned
    by the orchestrator. Agents are reported metadata on plain zsh panes."""
    snap = json.loads(herdr("api", "snapshot"))["result"]["snapshot"]
    if snap["workspaces"]:
        return
    j = lambda *a: json.loads(herdr(*a))["result"]
    ws = j("workspace", "create", "--label", "agent-rails", "--cwd", "/tmp", "--no-focus")
    orch = ws["root_pane"]["pane_id"]
    herdr("tab", "rename", ws["tab"]["tab_id"], "rails orchestrator")
    wid = ws["workspace"]["workspace_id"]
    spike = j("tab", "create", "--workspace", wid, "--label", "shell spike", "--cwd", "/tmp", "--no-focus")
    herdr("pane", "split", spike["root_pane"]["pane_id"], "--direction", "right")
    rec = j("tab", "create", "--workspace", wid, "--label", "recruiter", "--cwd", "/tmp", "--no-focus")["root_pane"]["pane_id"]
    wf1 = j("tab", "create", "--workspace", wid, "--label", "wf recruiter-2320", "--cwd", "/tmp", "--no-focus")["root_pane"]["pane_id"]
    wf2 = j("tab", "create", "--workspace", wid, "--label", "wf embed wave-a", "--cwd", "/tmp", "--no-focus")["root_pane"]["pane_id"]
    # Report agents only once each shell is up; a report made while the pane's
    # process is still starting is dropped by herdr's detection.
    for pane in (orch, rec, wf1, wf2):
        for _ in range(100):
            if "%" in herdr("pane", "read", pane, "--source", "visible", check=False):
                break
            time.sleep(0.05)
    for pane, agent, st in ((orch, "claude", "working"), (rec, "claude", "idle"), (wf1, "claude", "working"), (wf2, "codex", "blocked")):
        herdr("pane", "report-agent", pane, "--source", "spike", "--agent", agent, "--state", st)
    for pane in (wf1, wf2):
        herdr("pane", "report-metadata", pane, "--source", "spike", "--token", "host=PC")
    herdr("agent", "owner", "set", wf1, rec)
    herdr("agent", "owner", "set", wf2, orch)


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "up":
        up()
        seed()
    elif cmd == "env":
        for k, v in env().items():
            print(f"{k}={v}")
    elif cmd == "herdr":
        sys.stdout.write(herdr(*sys.argv[2:], check=False))
    elif cmd == "down":
        if os.path.exists(SOCK):
            r = subprocess.run([BIN, "--session", SESSION, "server", "stop"], env=env(), capture_output=True, text=True)
            print("stop:", r.returncode, r.stdout.strip(), r.stderr.strip())
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
