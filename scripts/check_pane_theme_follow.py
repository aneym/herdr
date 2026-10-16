#!/usr/bin/env python3
import json
import os
import pty
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import fcntl
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HERDR = os.environ.get("HERDR_BIN") or str(ROOT / "target" / "release" / "herdr")
COLORS = {"dark": ("cdd6f4", "1e1e2e"), "light": ("4c4f69", "eff1f5")}
SWITCHES = ["dark", "light", "dark", "light"]

PROBE = r"""
import os, re, sys, termios, tty, select, time
log = open(sys.argv[1], "a", buffering=1)
fd = sys.stdin.fileno()
tty.setraw(fd)
os.write(1, b"\x1b[?2031h")
buf = b""
pending = None
while True:
    r, _, _ = select.select([fd], [], [], 0.2)
    if not r:
        continue
    buf += os.read(fd, 4096)
    while True:
        m = re.search(rb"\x1b\[\?997;([12])n|\x1b\]11;rgb:([0-9a-f]{2})[0-9a-f]*/([0-9a-f]{2})[0-9a-f]*/([0-9a-f]{2})[0-9a-f]*(?:\x07|\x1b\\)", buf)
        if not m:
            buf = buf[-256:]
            break
        buf = buf[m.end():]
        if m.group(1):
            pending = "dark" if m.group(1) == b"1" else "light"
            os.write(1, b"\x1b]11;?\x1b\\")
        elif pending:
            log.write(pending + " " + (m.group(2) + m.group(3) + m.group(4)).decode() + "\n")
            pending = None
"""


def hex16(color):
    return "/".join(color[i:i + 2] * 2 for i in (0, 2, 4))


def cli(env, *args):
    out = subprocess.run([HERDR, *args], env=env, capture_output=True, text=True, timeout=20)
    if out.returncode != 0:
        raise SystemExit(f"herdr {' '.join(args)} failed: {out.stdout}{out.stderr}")
    return json.loads(out.stdout) if out.stdout.strip() else {}


def main():
    base = Path(tempfile.mkdtemp(prefix="herdr-theme-follow-", dir="/tmp"))
    config_home = base / "config"
    (config_home / "herdr").mkdir(parents=True)
    (config_home / "herdr" / "config.toml").write_text("onboarding = false\n")
    runtime = base / "run"
    runtime.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith("HERDR_")}
    env.update(XDG_CONFIG_HOME=str(config_home), XDG_RUNTIME_DIR=str(runtime),
               HERDR_SOCKET_PATH=str(base / "h.sock"), SHELL="/bin/sh")
    server = subprocess.Popen([HERDR, "server"], env=env, stdin=subprocess.DEVNULL,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              start_new_session=True)
    attach_pid = None
    try:
        deadline = time.time() + 15
        while not (base / "h.sock").exists():
            if time.time() > deadline:
                raise SystemExit("server socket never appeared")
            time.sleep(0.05)
        created = cli(env, "workspace", "create", "--cwd", str(base), "--no-focus")["result"]["root_pane"]
        probe_path = base / "probe.py"
        probe_path.write_text(PROBE)
        seen = base / "seen.log"
        cli(env, "pane", "run", created["pane_id"], f"{sys.executable} -I {probe_path} {seen}")

        mode = SWITCHES[0]
        attach_pid, fd = pty.fork()
        if attach_pid == 0:
            os.execvpe(HERDR, [HERDR, "terminal", "attach", created["terminal_id"], "--no-escape"], env)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
        host_reports = False
        expected = []
        switches = list(SWITCHES[1:])
        next_switch = time.time() + 3
        end = None
        while True:
            now = time.time()
            if end and now > end:
                break
            if switches and now >= next_switch and host_reports:
                mode = switches.pop(0)
                expected.append(mode)
                os.write(fd, b"\x1b[?997;%dn" % (1 if mode == "dark" else 2))
                next_switch = now + 1.5
                if not switches:
                    end = now + 2
            if now > next_switch + 10:
                raise SystemExit("attach client never enabled color scheme reports")
            ready, _, _ = select.select([fd], [], [], 0.05)
            if not ready:
                continue
            try:
                data = os.read(fd, 65536)
            except OSError:
                break
            if b"\x1b[?2031h" in data:
                host_reports = True
            for m in re.finditer(rb"\x1b\](1[01]);\?(?:\x07|\x1b\\)", data):
                fg, bg = COLORS[mode]
                color = fg if m.group(1) == b"10" else bg
                os.write(fd, b"\x1b]%s;rgb:%s\x1b\\" % (m.group(1), hex16(color).encode()))
            if b"\x1b[?996n" in data:
                os.write(fd, b"\x1b[?997;%dn" % (1 if mode == "dark" else 2))
            if b"\x1b[c" in data:
                os.write(fd, b"\x1b[?62;22c")

        lines = seen.read_text().split() if seen.exists() else []
        observed = [(lines[i], lines[i + 1]) for i in range(0, len(lines) - 1, 2)]
        failures = []
        for want in expected:
            hits = [bg for appearance, bg in observed if appearance == want]
            if not hits:
                failures.append(f"pane never saw a {want} report")
        for appearance, bg in observed:
            if bg != COLORS[appearance][1]:
                failures.append(f"pane told {appearance} but OSC 11 answered #{bg}")
        print(json.dumps({"expected": expected, "observed": observed, "failures": failures}))
        return 1 if failures else 0
    finally:
        if attach_pid:
            try:
                os.kill(attach_pid, signal.SIGKILL)
                os.waitpid(attach_pid, 0)
            except OSError:
                pass
        os.killpg(server.pid, signal.SIGKILL)
        server.wait()
        shutil.rmtree(base, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
