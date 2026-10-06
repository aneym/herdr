#!/usr/bin/env python3
"""Trackpad scroll check: a Herdr Shell pane scrolls one row per wheel report, as
Ghostty.app scrolls its own scrollback, not herdr's `ui.mouse_scroll_lines` (3).

  python3 scripts/check_scroll.py [--out checks/scroll.txt] [--space] [--attach-bin PATH]

Ghostty turns precise trackpad travel into one SGR wheel report per row. `herdr
terminal attach` used to scroll the server's pane `mouse_scroll_lines` rows per
report, so every step jumped 3 rows. The Shell now sets HERDR_ATTACH_SCROLL_LINES=1
for its attach processes.

Lab `shellspike-scroll`; pane 1 of "shell spike" holds 3000 numbered lines.
1. Host: `herdr terminal attach --no-escape` (HERDR_SHELL_BIN) under a pty, with
   the Shell's attach env and without; 10 wheel-up reports each. Asserts the
   server's scroll offset (`herdr pane get`) moved 1 row per report with the
   Shell env, and logs the report-to-frame round trip.
2. --space: the dev app in the Cua Space, attaching with --attach-bin (default
   HERDR_SHELL_BIN) over the forwarded lab socket. A trackpad swipe with momentum
   (scroll_gesture hook) runs; the hook's client-side frame log (top visible row
   per 1/120 s tick) goes next to --out. Asserts every event was precise and no
   frame moved more rows than wheel reports landed since the previous visible
   frame: a fast swipe coalesces reports into one frame, the old bug moved 3
   rows per report.
"""
import json
import os
import pty
import re
import select
import shutil
import struct
import subprocess
import sys
import time
import fcntl
import termios

os.environ["SHELL_LAB"] = "shellspike-scroll"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
SPACE = "--space" in sys.argv
if SPACE:
    os.environ["HERDR_SHELL_SPACE"] = "1"
ATTACH_BIN = (sys.argv[sys.argv.index("--attach-bin") + 1] if "--attach-bin" in sys.argv
              else os.environ["HERDR_SHELL_BIN"])
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lab as L  # noqa: E402
import scenario as S  # noqa: E402

if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "scroll.txt")
REPORT = b"\x1b[<64;10;10M"  # SGR wheel up at cell 10,10
lines, failures = [], []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def pane_info(pane):
    return json.loads(S.lab("herdr", "pane", "get", pane))["result"]["pane"]


def read_frames(fd, seconds):
    end, got = time.time() + seconds, []
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], max(0, end - time.time()))
        if not r:
            continue
        try:
            got.append((time.time(), len(os.read(fd, 65536))))
        except OSError:
            break
    return got


def host_attach(pane, env_lines):
    """Rows the server scrolled per wheel report, and the report-to-frame times."""
    info = pane_info(pane)
    m, s = pty.openpty()
    fcntl.ioctl(s, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 60, 0, 0))
    env = L.env()
    if env_lines:
        env["HERDR_ATTACH_SCROLL_LINES"] = env_lines
    p = subprocess.Popen([os.environ["HERDR_SHELL_BIN"], "--session", L.SESSION, "terminal", "attach",
                          info["terminal_id"], "--no-escape"],
                         stdin=s, stdout=s, stderr=s, env=env, start_new_session=True)
    os.close(s)
    try:
        read_frames(m, 1.5)
        start = pane_info(pane)["scroll"]["offset_from_bottom"]
        rtts = []
        for _ in range(10):
            t = time.time()
            os.write(m, REPORT)
            got = read_frames(m, 0.08)
            if got:
                rtts.append((got[0][0] - t) * 1000)
        time.sleep(0.3)
        moved = pane_info(pane)["scroll"]["offset_from_bottom"] - start
    finally:
        p.terminate()
        p.wait(5)
        os.close(m)
    return moved, rtts


def fill(pane):
    for _ in range(200):
        if "%" in S.pane_read(pane):
            break
        time.sleep(0.05)
    S.lab("herdr", "pane", "run", pane, "seq -f 'line %g' 1 3000")
    S.wait_read(pane, lambda x: "line 3000" in x, 10)


def space_gesture(spike, pane, out_log):
    app_copy = os.path.join(L.LAB, "app", "HerdrShell")
    os.makedirs(os.path.dirname(app_copy), exist_ok=True)
    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), app_copy + ".new")
    os.replace(app_copy + ".new", app_copy)
    os.environ["HERDR_SHELL_APP"] = app_copy
    os.environ["HERDR_SHELL_ATTACH_BIN"] = os.path.abspath(ATTACH_BIN)  # pushed into the guest
    say(f"app start (Cua Space): {S.app('start', '--herdr', S.guest_path(ATTACH_BIN)).strip()[:160]}")
    S.cmd({"cmd": "select", "tab": spike})
    # Synthesized events reach only a key window; the Space's desktop is the app's own.
    S.cmd({"cmd": "activate"})
    for _ in range(150):
        st = S.state()
        if any(x["pane"] == pane for x in st.get("surfaces", [])):
            break
        time.sleep(0.2)
    time.sleep(1.5)
    start = pane_info(pane)["scroll"]["offset_from_bottom"]
    # S.cmd maps a host "out" into the guest, waits for the file and pulls it back.
    S.cmd({"cmd": "scroll_gesture", "pane": pane, "dy": 12, "steps": 30, "momentum": 40, "out": out_log})
    moved = pane_info(pane)["scroll"]["offset_from_bottom"] - start
    shot = os.path.splitext(out_log)[0] + ".png"
    S.space("shot", shot)
    S.app("stop")
    return moved, open(out_log).read(), shot


def analyse(log):
    head = log.splitlines()[0]
    m = re.search(r"events=(\d+)", head)
    events = int(m.group(1)) if m else 0
    # One sample per tick; tick k delivers gesture event k while k < events.
    rows = []
    for tick, l in enumerate(log.splitlines()[1:]):
        ms, _, top = l.partition("\t")
        n = re.search(r"line (\d+)", top)
        if n:
            rows.append((tick, float(ms), int(n.group(1))))
    # (ms, rows moved, wheel reports landed since the previous visible frame)
    changes = [(t, prev - cur, max(0, min(k, events) - min(j, events)))
               for (j, _, prev), (k, t, cur) in zip(rows, rows[1:]) if cur != prev]
    return head, [(t, r) for _, t, r in rows], changes


def main():
    say(f"HerdrShell trackpad scroll check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    say(f"attach binary: {ATTACH_BIN}")
    if SPACE:
        S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"]
    spike = next(t["tab_id"] for t in snap["tabs"] if t["label"] == "shell spike")
    lay = next(x for x in snap["layouts"] if x["tab_id"] == spike)
    pane = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
    fill(pane)

    if not SPACE:
        moved, rtts = host_attach(pane, None)
        say(f"default config: 10 wheel reports scrolled {moved} rows")
        check("default attach keeps ui.mouse_scroll_lines (3 rows per report)", moved == 30, f"{moved}")
        moved, rtts = host_attach(pane, "1")
        say(f"Shell env (HERDR_ATTACH_SCROLL_LINES=1): 10 wheel reports scrolled {moved} rows")
        say("report-to-frame ms: " + " ".join(f"{x:.1f}" for x in rtts))
        check("Shell attach scrolls one row per wheel report", moved == 10, f"{moved}")
    else:
        out_log = os.path.splitext(S.OUT)[0] + "-frames.tsv"
        moved, log, shot = space_gesture(spike, pane, out_log)
        head, rows, changes = analyse(log)
        say(f"frame log: {out_log}  ({head})")
        say(f"screenshot: {shot}")
        say(f"server rows scrolled by the gesture: {moved}")
        jumps = [d for _, d, _ in changes]
        say(f"top-row changes: {len(changes)}; rows per change: {sorted(set(jumps))}; "
            f"first {rows[0][1] if rows else '?'} -> last {rows[-1][1] if rows else '?'}")
        gaps = [b[0] - a[0] for a, b in zip(changes, changes[1:])]
        over = [(round(t), d, n) for t, d, n in changes if d > n]
        if gaps:
            say(f"ms between visible changes: median {sorted(gaps)[len(gaps) // 2]:.1f}, max {max(gaps):.1f}")
        m = re.search(r"events=(\d+) precise=(\d+)", head)
        check("every gesture event reached the surface as precise", bool(m) and m.group(1) == m.group(2), head)
        check("the pane scrolled", moved > 0, f"{moved}")
        check("no visible frame moved more rows than wheel reports landed in it", bool(jumps) and not over,
              f"max {max(jumps) if jumps else None} rows; over (ms, rows, reports): {over[:5]}")
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(S.OUT), exist_ok=True)
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
