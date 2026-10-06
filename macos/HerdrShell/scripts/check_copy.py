#!/usr/bin/env python3
"""Copy check: text selected in a pane whose program owns the mouse (Claude Code, codex)
reaches the pasteboard with Cmd-C, as plain text any app can paste.

  python3 scripts/check_copy.py [--out checks/copy.txt]

Lab `shellspike-copy`. The app runs in the Cua Space (its own macOS guest) on a Studio
lab server. Pane 1 runs a stand-in agent that turns on SGR mouse reporting, prints two
known lines and counts the mouse reports it gets, so a plain drag goes to the program
and leaves no Ghostty selection, as in a Claude pane. Mouse events and Cmd-C go through
the app's window and Edit menu as for a physical mouse and key. Asserts on the guest's
general pasteboard (pbpaste in the guest) after: a plain drag, a double click on a word,
and a shift-drag (Ghostty's own selection). Each copy first puts a sentinel on the
pasteboard, so a Cmd-C that copies nothing fails.
"""
import json
import os
import re
import shlex
import shutil
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-copy"
os.environ.setdefault("HERDR_SHELL_SPACE", "1")
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LABDIR = os.path.expanduser(f"~/.cache/herdr-build/{os.environ['SHELL_LAB']}")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
APP_SRC = os.environ.get("CHECK_COPY_APP") or os.path.join(D0, ".build", "release", "HerdrShell")
AGENT = os.path.join(LABDIR, "mouse_agent.py")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "copy.txt")
lines, failures = [], []
SENTINEL = "copy-check-sentinel"

AGENT_SRC = r'''import os, sys, termios, tty
tty.setraw(0)
w = sys.stdout.write
w("\x1b[2J\x1b[1;1HCOPY-ALPHA first line of agent text\r\nCOPY-BETA second line here\r\n")
w("\x1b[?1000h\x1b[?1002h\x1b[?1006h")
sys.stdout.flush()
n = 0
while True:
    b = os.read(0, 4096)
    if not b:
        break
    n += b.count(b"\x1b[<")
    w("\x1b[5;1HREPORTS %d\x1b[K" % n)
    sys.stdout.flush()
'''


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def herdr(*a):
    return S.lab("herdr", *a)


def wait_state(pred, timeout=25):
    t0, last = time.time(), None
    while time.time() - t0 < timeout:
        try:
            last = S.state()
        except (SystemExit, RuntimeError):
            time.sleep(0.2)
            continue
        if pred(last):
            return last
        time.sleep(0.2)
    return last


def surface(s, pane):
    return next((x for x in (s or {}).get("surfaces", []) if x["pane"] == pane), {})


def pasteboard():
    return S.space("exec", "pbpaste")


def reports(pane):
    m = re.search(r"REPORTS (\d+)", S.pane_read(pane))
    return int(m.group(1)) if m else 0


def mouse(pane, action, col, row, mods=(), clicks=1):
    S.cmd({"cmd": "mouse", "pane": pane, "action": action, "col": col, "row": row,
           "mods": list(mods), "clicks": clicks})


def copy_after(gesture):
    """Sentinel on the pasteboard, the gesture, Cmd-C; returns what the pasteboard holds."""
    S.space("exec", f"printf %s {shlex.quote(SENTINEL)} | pbcopy")
    gesture()
    time.sleep(0.3)
    S.key("c", ["cmd"])
    for _ in range(20):
        got = pasteboard()
        if got != SENTINEL:
            return got
        time.sleep(0.1)
    return got


def main():
    say(f"HerdrShell copy check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    with open(AGENT, "w") as f:
        f.write(AGENT_SRC)
    snap = json.loads(herdr("api", "snapshot"))["result"]["snapshot"]
    tabs = {x["tab_id"]: x["label"] for x in snap["tabs"]}
    spike = next(k for k, v in tabs.items() if v == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == spike)
    p1 = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
    for _ in range(200):
        if "%" in S.pane_read(p1):
            break
        time.sleep(0.05)
    herdr("pane", "run", p1, f"/usr/bin/python3 {AGENT}")
    S.wait_read(p1, lambda x: "COPY-BETA" in x, 10)

    shutil.copy2(APP_SRC, APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app {APP_SRC}")
    say(f"app start (Cua Space guest, lab socket forwarded from Studio): {S.app('start').strip()[:160]}")
    S.cmd({"cmd": "select", "tab": spike})
    s = wait_state(lambda s: s.get("focused_pane") == p1 and surface(s, p1).get("mouse_captured")
                   and any("COPY-BETA" in l for l in surface(s, p1).get("visible_nonblank", [])), 60)
    check("the lab pane's program owns the mouse in the guest Shell", bool(surface(s, p1).get("mouse_captured")),
          f"focused={None if s is None else s.get('focused_pane')} mouse_captured={surface(s, p1).get('mouse_captured')}")

    # 1. Plain drag from the first cell to the end of COPY-BETA: the program gets it.
    before = reports(p1)

    def drag():
        mouse(p1, "down", 0, 0)
        for col in (5, 12, 4):
            mouse(p1, "drag", col, 1)
        mouse(p1, "drag", 8, 1)
        mouse(p1, "up", 8, 1)
    got = copy_after(drag)
    after = reports(p1)
    s = S.state()
    check("the plain drag went to the program, not to a Ghostty selection",
          after > before and surface(s, p1).get("selection") is None,
          f"reports {before}->{after}, ghostty selection={surface(s, p1).get('selection')!r}")
    check("Cmd-C after a plain drag puts the dragged text on the pasteboard",
          got == "COPY-ALPHA first line of agent text\nCOPY-BETA", repr(got))

    # 2. Double click on a word.
    got = copy_after(lambda: (mouse(p1, "down", 13, 1, clicks=1), mouse(p1, "up", 13, 1, clicks=1),
                              mouse(p1, "down", 13, 1, clicks=2), mouse(p1, "up", 13, 1, clicks=2)))
    check("Cmd-C after a double click copies the word", got == "second", repr(got))

    # 3. Shift-drag: Ghostty's own selection, copied through the same Edit menu path.
    seen = {}

    def shift_drag():
        mouse(p1, "down", 0, 0, ["shift"])
        mouse(p1, "drag", 9, 0, ["shift"])
        mouse(p1, "up", 9, 0, ["shift"])
        time.sleep(0.2)
        seen["selection"] = surface(S.state(), p1).get("selection")
    got = copy_after(shift_drag)
    sel = seen["selection"]
    check("Cmd-C after a shift-drag copies Ghostty's selection",
          bool(sel) and "ALPHA" in sel and got == sel, f"selection={sel!r} pasteboard={got!r}")

    shot = os.path.join(LABDIR, "copy.png")
    S.cmd({"cmd": "shot", "out": shot})
    say(f"screenshot: {shot}")
    finish()


def finish():
    S.app("stop")
    time.sleep(0.4)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(S.OUT), exist_ok=True)
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
