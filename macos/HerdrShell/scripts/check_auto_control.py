#!/usr/bin/env python3
"""Auto take control check: two herdr clients on one terminal, no menu action.

  HERDR_SHELL_SPACE=1 python3 scripts/check_auto_control.py --out checks/auto-control.txt

Alex, 2026-10-10: "take control should jsut automatically happen in all clients instead of
me having to click the mneu button when we have focus". Client A is this build of Herdr
Shell (in the Cua Space, on its own lab session shellspike-atc); client B is a CLI
`herdr terminal attach --takeover` in a pty on the host, the other machine's stand-in.
The check never sends the app's reclaim command or clicks Take back.

  1. A types; B takes the pane over and types; A types again: A's first key takes the
     pane back, B is shut down, and A's whole line (first key included) reaches the pane.
  2. Settle window: B takes the pane and keeps typing a line, one key per 300 ms. A types
     its line in the middle of it. A does not take the pane until B has been quiet for
     1.5 s; B's line arrives whole, then A's line arrives whole after it.
"""
import json
import os
import pty
import select
import subprocess
import sys
import threading
import time

os.environ["SHELL_LAB"] = "shellspike-atc"
os.environ.setdefault("HERDR_SPACE_OWNER", "herdr-auto-take-control")
# The lab runs the installed fork binary unless told otherwise (it reports human_input_age_ms).
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

D = S.D
LAB = S.LAB
os.makedirs(LAB, exist_ok=True)

def lab_bin():
    """The lab's own herdr copy (lab.py may use a short hashed dir, so ask it)."""
    env = dict(l.split("=", 1) for l in S.lab("env").splitlines() if "=" in l)
    return os.path.join(env["PATH"].split(":")[0], "herdr")

SCRATCH = os.path.expanduser("~/.cache/herdr-build/atc-swift")
APP_BIN = os.path.join(SCRATCH, "release", "HerdrShell")
SETTLE = 1.5
OUT = os.path.join(D, "checks", "auto-control.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
lines, failures = [], []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def wait(pred, timeout, step=0.02):
    t0 = time.time()
    while time.time() - t0 < timeout:
        v = pred()
        if v:
            return v, time.time() - t0
        time.sleep(step)
    return None, None


def state():
    return S.state()


def lc(s, tid):
    return s["lifecycle"]["panes"].get(tid) or {}


def typ(t):
    S.cmd({"cmd": "type", "text": t})


def key(k):
    S.cmd({"cmd": "key", "key": k, "mods": []})


def has_line(pane, text):
    return lambda: any(l.strip() == text for l in S.pane_read(pane).splitlines())


class Cli:
    """Client B: a takeover attach in a pty whose output is drained until it exits."""

    def __init__(self, terminal):
        env = {l.split("=", 1)[0]: l.split("=", 1)[1] for l in S.lab("env").splitlines() if "=" in l}
        self.master, slave = pty.openpty()
        subprocess.run(["stty", "rows", "30", "cols", "100"], stdin=slave)
        self.proc = subprocess.Popen([lab_bin(), "--session", "shellspike-atc", "terminal", "attach", terminal,
                                      "--takeover", "--no-escape"],
                                     stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        self.exited_at = None
        self.out = b""
        threading.Thread(target=self._drain, daemon=True).start()

    def _drain(self):
        while True:
            if self.proc.poll() is not None and self.exited_at is None:
                self.exited_at = time.time()
            r, _, _ = select.select([self.master], [], [], 0.02)
            if r:
                try:
                    chunk = os.read(self.master, 65536)
                except OSError:
                    chunk = b""
                if not chunk and self.proc.poll() is not None:
                    break
                self.out += chunk
            if self.proc.poll() is not None and self.exited_at is None:
                self.exited_at = time.time()
            if self.exited_at and time.time() - self.exited_at > 0.5:
                break

    def write(self, b):
        os.write(self.master, b)

    def close(self):
        if self.proc.poll() is None:
            self.proc.kill()
        try:
            os.close(self.master)
        except OSError:
            pass


def held(tid):
    return lambda: lc(state(), tid).get("state") == "held"


def running(tid):
    return lambda: lc(state(), tid).get("state") == "running"


def finish():
    S.app("stop")
    S.lab("down")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL (' + ', '.join(failures) + ')'}")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    open(OUT, "w").write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


def main():
    say(f"Herdr Shell auto take control check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    head = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=D, capture_output=True, text=True).stdout.strip()
    say(f"tree {head} + working changes; settle window {SETTLE}s; space={S.SPACE}")
    r = subprocess.run(["swift", "build", "-c", "release", "--scratch-path", SCRATCH], cwd=D, capture_output=True, text=True)
    say(f"swift build: {'ok' if r.returncode == 0 else 'FAILED'}")
    if r.returncode != 0:
        say(r.stdout[-1500:] + r.stderr[-1500:])
        return finish()
    os.environ["HERDR_SHELL_APP"] = APP_BIN
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    tabs = {t["label"]: t["tab_id"] for t in snap["tabs"]}
    lay = next(l for l in snap["layouts"] if l["tab_id"] == tabs["shell spike"])
    p1 = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
    t1 = {p["pane_id"]: p["terminal_id"] for p in snap["panes"]}[p1]
    say(f"lab session shellspike-atc; pane {p1} (terminal {t1})")
    say(S.app("start").strip())

    def ready():
        s = state()
        x = next((v for v in s["surfaces"] if v["pane"] == p1), None)
        return s if x and any("%" in l for l in x["visible_nonblank"]) and s["focused_pane"] == p1 else None

    s0, _ = wait(ready, 40, 0.1)
    check("client A (app) attached to the pane with focus", s0 is not None)
    if not s0:
        return finish()

    # ---- 1. type in each client in turn ---------------------------------------------------
    say()
    say("== 1. each client types in turn; no menu action")
    typ("echo atc-A1")
    key("return")
    check("A's keys reach the pane", wait(has_line(p1, "atc-A1"), 6)[0] is not None)
    b = Cli(t1)
    check("B takes the pane over (A shows it held)", wait(held(t1), 6)[0] is not None)
    time.sleep(0.3)
    b.write(b"echo atc-B1\r")
    check("B's keys reach the pane", wait(has_line(p1, "atc-B1"), 6)[0] is not None)
    time.sleep(SETTLE + 0.3)
    t_type = time.time()
    typ("echo atc-A2")
    key("return")
    back, _ = wait(running(t1), 6)
    check("A's first key takes the pane back with no menu action", back is not None,
          f"seen from the host {time.time() - t_type:.2f}s, Space ssh hops included" if back else json.dumps(lc(state(), t1)))
    check("B is shut down by A's take", wait(lambda: b.proc.poll() is not None, 6)[0] is not None)
    say(f"   app-side: first key on the held pane -> queued keys replayed: {lc(state(), t1).get('last_take_s')}s")
    ok, dt = wait(has_line(p1, "atc-A2"), 6)
    check("A's whole line, first key included, reaches the pane", ok is not None,
          f"{dt:.2f}s after the take" if ok else S.pane_read(p1)[-300:])
    b.close()

    # ---- 2. settle window -----------------------------------------------------------------
    say()
    say("== 2. settle window: a client that is typing is not cut off")
    b = Cli(t1)
    check("B takes the pane over again", wait(held(t1), 6)[0] is not None)
    time.sleep(0.3)
    b_line = "echo atc-B2-settle"
    sent = {"last": None}

    def b_types():
        for ch in b_line:
            b.write(ch.encode())
            sent["last"] = time.time()
            time.sleep(0.3)
        b.write(b"\r")
        sent["last"] = time.time()

    th = threading.Thread(target=b_types)
    th.start()
    time.sleep(1.2)  # B is mid-line
    t_a = time.time()
    typ("echo atc-A3")
    key("return")
    th.join()
    took, _ = wait(lambda: b.exited_at or (time.time() if b.proc.poll() is not None else None), 12, 0.01)
    if took:
        gap = took - sent["last"]
        say(f"   A typed {t_a - (sent['last'] - 0.3 * len(b_line)):.1f}s into B's line; "
            f"B's last key -> B shut down: {gap:.2f}s")
        check(f"A waited until B was quiet for {SETTLE}s before taking the pane", gap >= SETTLE - 0.05, f"{gap:.2f}s")
        check("A did not wait much longer than the settle window", gap < SETTLE + 1.5, f"{gap:.2f}s")
    else:
        check("A took the pane after B paused", False, json.dumps(lc(state(), t1)))
    ok, _ = wait(has_line(p1, "atc-A3"), 8)
    txt = S.pane_read(p1)
    rows = [l.strip() for l in txt.splitlines()]
    check("B's line arrived whole", "atc-B2-settle" in rows, txt[-300:].replace("\n", " | "))
    check("A's queued line arrived whole, after B's", ok is not None and "atc-B2-settle" in rows
          and rows.index("atc-A3") > rows.index("atc-B2-settle"), txt[-300:].replace("\n", " | "))
    check("no line mixes A's and B's keys", not any(("atc-A" in l and "B2" in l) for l in rows))
    b.close()

    # ---- 3. A keeps working after the takes -----------------------------------------------
    say()
    typ("echo atc-A4")
    key("return")
    check("A types normally after the takes", wait(has_line(p1, "atc-A4"), 6)[0] is not None)
    s = state()
    l = lc(s, t1)
    say(f"   app lifecycle: state={l.get('state')} respawns={l.get('respawns')} queued_keys={l.get('queued_keys')} gated={l.get('gated')}")
    check("nothing left queued or gated", l.get("queued_keys") == 0 and not l.get("gated"))
    S.check_front(check)
    finish()


if __name__ == "__main__":
    main()
