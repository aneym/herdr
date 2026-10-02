#!/usr/bin/env python3
"""P8 check: mouse, scroll, selection, copy. Lab session only (SHELL_LAB, default
shellspike-p8). Usage: check_p8.py [--out checks/P8.txt]

  1. after `seq 1 500`, wheel up brings line 1 into view (`herdr pane read --source visible`)
  2. shift+drag selects text natively and Cmd+C puts it on the system pasteboard
     (the attach client keeps the mouse captured so wheel reaches herdr's scrollback;
     shift is Ghostty's standard override, plain drag stays with the program)
  3. `vim` with `set mouse=a` moves its cursor when a cell is clicked

Mouse events are NSEvents dispatched to the app's window (window.sendEvent); keys
are CGEvents posted to the app's pid. The system pasteboard is saved and restored.
"""
import os
import subprocess
import sys
import time

os.environ.setdefault("SHELL_LAB", "shellspike-p8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as sc  # noqa: E402

D = sc.D
OUT = os.path.join(D, "checks", "P8.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])


def visible(pane):
    return sc.lab("herdr", "pane", "read", pane, "--source", "visible")


def wait_visible(pane, pred, timeout=5.0):
    t0 = time.time()
    while time.time() - t0 < timeout:
        txt = visible(pane)
        if pred(txt):
            return txt, time.time() - t0
        time.sleep(0.05)
    return visible(pane), None


def mouse(pane, action, col, row, mods=(), button="left"):
    sc.cmd({"cmd": "mouse", "pane": pane, "action": action, "col": col, "row": row,
            "mods": list(mods), "button": button})


def surface(pane):
    return next(x for x in sc.state()["surfaces"] if x["pane"] == pane)


def pasteboard():
    return subprocess.run(["pbpaste"], capture_output=True, text=True).stdout


def set_pasteboard(text):
    subprocess.run(["pbcopy"], input=text, text=True)


def main():
    import json
    sc.say(f"HerdrShell P8 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    sc.app("stop")
    sc.lab("down")
    time.sleep(0.5)
    sc.lab("up")
    snap = sc.herdr_json("api", "snapshot")["result"]["snapshot"]
    tabs = {x["tab_id"]: x["label"] for x in snap["tabs"]}
    spike = next(k for k, v in tabs.items() if v == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == spike)
    p1 = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
    sc.say(f"lab session '{sc.NAME}', pane under test {p1} (plain zsh)")
    sc.say(f"app: {sc.app('start').strip()}")
    saved = pasteboard()
    try:
        ready = False
        for _ in range(200):
            s = sc.state()
            x = {x["pane"]: x for x in s["surfaces"]}.get(p1)
            if x and any("%" in l for l in x["visible_nonblank"]):
                ready = True
                break
            time.sleep(0.05)
        sc.check("pane attached and rendered by a Ghostty surface", ready)
        # 1. Wheel to herdr scrollback.
        sc.type_("seq 1 500")
        sc.key("return")
        txt, _ = wait_visible(p1, lambda t: any(l.strip() == "500" for l in t.splitlines()))
        lines = [l.strip() for l in txt.splitlines()]
        sc.check("after seq 1 500 the visible screen ends at 500 and line 1 is off screen",
                 "500" in lines and "1" not in lines)
        mouse(p1, "move", 10, 10)
        n = 0
        seen = None
        t0 = time.time()
        while n < 60 and seen is None:
            sc.cmd({"cmd": "scroll", "pane": p1, "dy": 20})
            n += 1
            _, seen = wait_visible(p1, lambda t: "1" in [l.strip() for l in t.splitlines()], timeout=0.4)
        top = [l.strip() for l in visible(p1).splitlines() if l.strip()][:3]
        sc.check("wheel up shows line 1 (herdr pane read --source visible)", seen is not None,
                 f"after {n} wheel events; top of screen {top}")

        # 2. Selection and copy.
        sc.type_("clear; echo alpha bravo charlie")
        sc.key("return")
        wait_visible(p1, lambda t: t.lstrip().startswith("alpha bravo charlie"))
        set_pasteboard("P8-SENTINEL")
        mouse(p1, "down", 0, 0)
        mouse(p1, "drag", 6, 0)
        mouse(p1, "up", 6, 0)
        time.sleep(0.3)
        x = surface(p1)
        sc.check("plain drag is left to the program (no native selection, mouse captured for herdr)",
                 x["selection"] is None and x["mouse_captured"], f"selection={x['selection']!r} captured={x['mouse_captured']}")
        mouse(p1, "down", 0, 0, ["shift"])
        mouse(p1, "drag", 4, 0, ["shift"])
        mouse(p1, "drag", 10, 0, ["shift"])
        mouse(p1, "up", 10, 0, ["shift"])
        time.sleep(0.3)
        sel = surface(p1)["selection"]
        sc.check("shift+drag selects text in the surface", bool(sel) and "alpha bravo charlie".startswith(sel.strip()) and len(sel.strip()) >= 8,
                 f"selection={sel!r}")
        sc.key("c", ["cmd"])
        pb = None
        t0 = time.time()
        while time.time() - t0 < 3:
            pb = pasteboard()
            if pb != "P8-SENTINEL":
                break
            time.sleep(0.05)
        sc.check("Cmd+C puts the selected text on the pasteboard", sel is not None and pb == sel, f"pasteboard={pb!r}")
        txt = visible(p1)
        last = [l for l in txt.splitlines() if l.strip()][-1].strip()
        sc.check("Cmd+C sent nothing into the pane (prompt line still empty)", last.endswith("%") and "^C" not in txt,
                 f"last line {last!r}")

        # 3. vim with mouse=a.
        sc.type_("clear; seq 101 160 > /tmp/p8_vim.txt; vim -u NONE -N /tmp/p8_vim.txt")
        sc.key("return")
        wait_visible(p1, lambda t: "101" in t and "~" not in t.splitlines()[0])
        sc.type_(":set mouse=a ttymouse=sgr")
        sc.key("return")
        time.sleep(0.3)
        results = []
        for (col, row, want) in ((2, 6, "7,3"), (1, 20, "21,2"), (0, 2, "3,1")):
            mouse(p1, "down", col, row)
            mouse(p1, "up", col, row)
            time.sleep(0.3)
            sc.type_(':echo line(".").",".col(".")')
            sc.key("return")
            txt, dt = wait_visible(p1, lambda t, w=want: [l.strip() for l in t.splitlines() if l.strip()][-1] == w, timeout=3)
            results.append((col, row, want, dt is not None))
        sc.check("vim (mouse=a) cursor follows clicks", all(r[3] for r in results),
                 "; ".join(f"click col {c} row {r} -> {w} {'ok' if ok else 'MISSING'}" for c, r, w, ok in results))
        sc.type_(":q!")
        sc.key("return")
        wait_visible(p1, lambda t: "~" not in t)
    finally:
        set_pasteboard(saved)
        sc.say("pasteboard restored")
        sc.check_front(sc.check)
        sc.app("stop")
        time.sleep(0.5)
        sc.say(f"lab down: {sc.lab('down').strip()}")
    sc.say()
    sc.say(f"RESULT: {'PASS' if not sc.failures else 'FAIL ' + ', '.join(sc.failures)}")
    with open(OUT, "w") as f:
        f.write("\n".join(sc.lines) + "\n")
    sys.exit(1 if sc.failures else 0)


if __name__ == "__main__":
    main()
