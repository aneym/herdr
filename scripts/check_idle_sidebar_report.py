#!/usr/bin/env python3
"""Check: an idle sidebar client keeps its sidebar report fresh.

The factory sections check reads ~/.local/state/herdr/client-shell/sidebar-*.json and calls a
report older than 180 s client_offline. A client with no input drew no frames, so its report went
220 s stale (2026-10-09). This runs the real binary as an isolated server plus one TUI client in a
pty, with a fake idle claude agent tagged in the factory overlay, sends no input, and fails if the
report gets older than MAX_AGE_S within WINDOW_S.

  python3 scripts/check_idle_sidebar_report.py [HERDR_BINARY]   (default ~/.local/bin/herdr)
"""
import fcntl, json, os, pty, shutil, struct, subprocess, sys, tempfile, termios, threading, time
from pathlib import Path

SOURCE = Path(sys.argv[1] if len(sys.argv) > 1 else "~/.local/bin/herdr").expanduser()
DURATION = float(os.environ.get("WINDOW_S", "90"))
MAX_AGE_S = 75
WINSZ = struct.pack("HHHH", 50, 160, 0, 0)

root = Path(tempfile.mkdtemp(prefix="hi-", dir="/tmp"))
home, config = root / "home", root / "config"
home.mkdir(); (config / "herdr").mkdir(parents=True)
overlay = root / "overlay.json"
overlay.write_text('{"version":1,"tabs":{}}\n')
(config / "herdr/config.toml").write_text(
    'onboarding = false\n[session]\nresume_agents_on_restore = false\n\n'
    '[ui]\nagent_panel_sort = "tree"\n\n[ui.factory]\nenabled = true\n'
    f'overlay_file = "{overlay}"\n')
binary = root / "herdr"; shutil.copy2(SOURCE, binary)
bindir = root / "bin"; bindir.mkdir()
(root / "claude.c").write_text("#include <unistd.h>\nint main(){sleep(900);return 0;}\n")
subprocess.run(["/usr/bin/cc", "-o", str(bindir / "claude"), str(root / "claude.c")], check=True)
env = {"HOME": str(home), "XDG_CONFIG_HOME": str(config), "XDG_STATE_HOME": str(root / "state"),
       "XDG_CACHE_HOME": str(root / "cache"), "XDG_DATA_HOME": str(root / "data"),
       "HERDR_SOCKET_PATH": str(root / "s.sock"), "HERDR_CLIENT_SOCKET_PATH": str(root / "c.sock"),
       "PATH": f"{bindir}:/usr/bin:/bin:/usr/sbin:/sbin", "SHELL": "/bin/sh", "TERM": "xterm-256color",
       "LANG": "en_US.UTF-8", "TMPDIR": str(root)}
assert env["HERDR_SOCKET_PATH"] != str(Path.home() / ".config/herdr/herdr.sock")
reports = root / "state/herdr/client-shell"

def cli(*a):
    r = subprocess.run([str(binary), *a], env=env, capture_output=True, text=True, timeout=8)
    return json.loads(r.stdout)["result"] if r.returncode == 0 and r.stdout.strip().startswith("{") else None

server = subprocess.Popen([str(binary), "server"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
pid = None
status = 1
try:
    deadline = time.time() + 20
    made = None
    while time.time() < deadline and not made:
        time.sleep(0.5)
        made = cli("workspace", "create", "--label", "idle", "--cwd", str(root))
    tab, pane = made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    time.sleep(1.0)
    deadline = time.time() + 20
    while time.time() < deadline:
        agents = (cli("agent", "list") or {}).get("agents") or []
        if any(a.get("pane_id") == pane for a in agents):
            break
        cli("pane", "run", pane, f"{bindir / 'claude'}")
        time.sleep(2)
    else:
        raise SystemExit("fake claude not detected")
    overlay.write_text(json.dumps({"version": 1, "tabs": {tab: {"kind": "lane", "section": "implementing", "name": "I"}}}))
    time.sleep(2.2)
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, WINSZ)
        os.environ.clear(); os.environ.update(env)
        os.execv(str(binary), [str(binary)])
    fcntl.ioctl(fd, termios.TIOCSWINSZ, WINSZ)
    def drain():
        while True:
            try:
                c = os.read(fd, 65536)
            except OSError: return
            if not c: return
    threading.Thread(target=drain, daemon=True).start()
    # Redraw until a frame shows the tagged tab, then send no input at all.
    def rows_in(path):
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            return 0
        return sum(len(w.get("tabs") or []) for w in data.get("workspaces") or [])
    deadline = time.time() + 20
    report = None
    while time.time() < deadline:
        time.sleep(0.8)
        found = sorted(reports.glob("sidebar-*.json"))
        if found and rows_in(found[0]):
            report = found[0]
            break
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 50, 161, 0, 0)); time.sleep(0.2)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, WINSZ)
    if report is None:
        raise SystemExit("FAIL: the client wrote no sidebar report with the tagged tab")
    start = time.time(); max_age = 0; writes = set(); samples = []
    while time.time() - start < DURATION:
        try:
            at = json.loads(report.read_text())["at"]; writes.add(at)
        except (OSError, ValueError, KeyError):
            at = None
        rows = rows_in(report)
        age = (time.time() * 1000 - at) / 1000 if at else None
        if age is not None: max_age = max(max_age, age)
        samples.append((round(time.time() - start), None if age is None else round(age, 1), rows))
        time.sleep(5)
    print(json.dumps({"binary": str(SOURCE), "duration_s": DURATION, "distinct_writes": len(writes), "max_age_s": round(max_age, 1),
                      "samples": samples}))
    ok = max_age <= MAX_AGE_S and all(age is not None and rows for _, age, rows in samples)
    print(("PASS" if ok else "FAIL") + f": idle client report max age {max_age:.1f} s (limit {MAX_AGE_S} s over {DURATION:.0f} s)")
    status = 0 if ok else 1
finally:
    if pid:
        os.kill(pid, 15)
    subprocess.run([str(binary), "server", "stop"], env=env, capture_output=True, timeout=10)
    server.terminate(); time.sleep(0.5)
    shutil.rmtree(root, ignore_errors=True)
sys.exit(status)
