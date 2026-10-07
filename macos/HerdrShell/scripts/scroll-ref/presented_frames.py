#!/usr/bin/env python3
"""Presented-frame scroll comparison, Herdr Shell vs Ghostty.app, in the Cua Space.

  HERDR_SPACE_OWNER=<you> python3 scripts/scroll-ref/presented_frames.py OUT_DIR [RUNS]

One Space hold. Ghostty.app (scrollback of 3000 numbered lines, the Shell's font config)
and then the dev Shell (.build/release/HerdrShell, a lab pane with the same lines) each get
RUNS (default 3) std swipes posted to the HID tap by inject.swift, the delivery a real
trackpad uses. While each swipe runs, framecap.swift records the same size rect of
the terminal (300x300 pt) through ScreenCaptureKit: one entry per frame the window server presented,
with how far the content moved. Raw logs go to OUT_DIR; the summary to OUT_DIR/summary.txt.
Ghostty is confirmed gone before the Shell starts; the lock is kept if it is not.
HERDR_SHELL_BIN picks the herdr server and attach binary (default ~/.local/bin/herdr).
Exits 1 when any run captured no motion or fewer than MIN_MOVING moving frames.
"""
import json
import os
import re
import shutil
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
os.environ["SHELL_LAB"] = "shellspike-frames"
os.environ["HERDR_SHELL_SPACE"] = "1"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
import space as SP  # noqa: E402
import scenario as S  # noqa: E402

G = SP.GUEST_HOME + "/.herdr-space/scrollref"
STD = (12, 30, 40, 0.92)  # check_scroll.PROFILES["std"]
CAPTURE_S = 2.0
W, H = 300, 300
MIN_MOVING = 10  # a std swipe moves the content on about 20 presented frames
GHOSTTY_ARGS = ("--font-family='SF Mono' --font-size=13.5 --adjust-cell-height=8% --window-position-x=80 "
                "--window-position-y=60 --window-width=100 --window-height=36 --confirm-close-surface=false")


def say(text, log):
    print(text, flush=True)
    log.append(text)


def swipe(x, y, label, out, log):
    """framecap in the background, then one std swipe at x,y; pulls the frame log."""
    guest = f"{G}/{label}.tsv"
    dy, steps, momentum, decay = STD
    SP.gexec(f"rm -f {guest}; cd {G} && (nohup ./framecap {x - W // 2} {y - H // 2} {W} {H} {CAPTURE_S} {guest} "
             f"> {G}/{label}.cap.log 2>&1 &) ; sleep 0.4; ./inject 0 {x} {y} {dy} {steps} {momentum} {decay} 8.3333 hid; "
             f"sleep {CAPTURE_S}; for i in 1 2 3 4 5 6 7 8 9 10; do [ -s {guest} ] && break; sleep 0.3; done")
    local = os.path.join(out, f"{label}.tsv")
    SP.pull(guest, local)
    say(f"{label}: {open(local).readline().strip()}", log)


def summarize(path):
    rows = [line.split("\t") for line in open(path).read().splitlines()[1:]]
    frames = [(float(t), int(s), float(e), kind) for t, s, e, kind in rows]
    moving = [f for f in frames if f[3] == "frame" and f[1] != 0]
    if not moving:
        return None
    start, end = moving[0][0], moving[-1][0]
    steps = [abs(f[1]) for f in moving]
    gaps = [b[0] - a[0] for a, b in zip(moving, moving[1:])]
    still = [f for f in frames if start < f[0] < end and (f[3] == "idle" or f[1] == 0)]
    return {"moving_frames": len(moving), "span_ms": round(end - start, 1), "px_total": sum(steps),
            "px_per_frame": {n: steps.count(n) for n in sorted(set(steps))}, "max_px": max(steps),
            "longest_gap_ms": round(max(gaps), 1) if gaps else 0,
            "refreshes_without_motion": len(still), "max_err": round(max(f[2] for f in moving), 3)}


def ghostty_gone():
    r = SP.gexec("pgrep -x ghostty", check=False)
    return r.returncode == 1 and not r.stdout.strip()


def main():
    out = os.path.abspath(sys.argv[1])
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    os.makedirs(out, exist_ok=True)
    log = []
    fresh = SP.lock_take(SP.owner_name(), 240)
    t0 = time.time()
    shell_started = False
    try:
        SP.up()
        SP.gexec(f"mkdir -p {G}")
        for name in ("inject.swift", "framecap.swift"):
            SP.push(os.path.join(HERE, name), f"{G}/{name}")
        r = SP.gexec(f"cd {G} && rm -f inject framecap && swiftc -O inject.swift -o inject && "
                     f"swiftc -O framecap.swift -o framecap && shasum inject framecap && "
                     f"echo 'import AppKit; print(NSScreen.screens[0].frame.height)' > screen.swift && "
                     f"swiftc screen.swift -o screen && ./screen && "
                     f"defaults read $HOME/Applications/Ghostty.app/Contents/Info.plist CFBundleShortVersionString")
        say("guest compile:\n" + r.stdout.strip(), log)
        screen_h = float(r.stdout.split()[-2])

        # Ghostty.app: scrollback, no mouse reporting, so the swipe scrolls its own viewport.
        cmd = "/bin/sh -c 'seq -f \\\"line %g\\\" 1 3000; exec sleep 900'"
        SP.gexec(f"pkill -x ghostty; sleep 0.5; open -na $HOME/Applications/Ghostty.app --args {GHOSTTY_ARGS} "
                 f"--command=\"{cmd}\"", check=False)
        time.sleep(4)
        SP.run(["cua", "sb", "screenshot", SP.REF, "-o", os.path.join(out, "ghostty.png")])
        for i in range(runs):
            swipe(250, 330, f"ghostty-{i + 1}", out, log)  # the rect starts at the text's left edge
            time.sleep(1)
        SP.gexec("pkill -x ghostty; sleep 0.8", check=False)
        if not ghostty_gone():
            raise SystemExit("Ghostty may still run in the Space; the lock is kept")

        # Herdr Shell: the dev build on a lab pane of the same lines.
        S.lab("down")
        S.lab("up")
        snap = json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"]
        spike = next(t["tab_id"] for t in snap["tabs"] if t["label"] == "shell spike")
        lay = next(x for x in snap["layouts"] if x["tab_id"] == spike)
        pane = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
        for _ in range(200):
            if "%" in S.pane_read(pane):
                break
            time.sleep(0.05)
        S.lab("herdr", "pane", "run", pane, "seq -f 'line %g' 1 3000")
        S.wait_read(pane, lambda x: "line 3000" in x, 10)
        shell_started = True
        # As check_scroll: scenario pushes this attach binary into the guest for --herdr.
        os.environ["HERDR_SHELL_ATTACH_BIN"] = os.path.abspath(os.environ["HERDR_SHELL_BIN"])
        say("app start: " + S.app("start", "--herdr", S.guest_path(os.environ["HERDR_SHELL_BIN"])).strip()[:160], log)
        S.cmd({"cmd": "select", "tab": spike})
        S.cmd({"cmd": "activate"})
        for _ in range(150):
            if any(x["pane"] == pane for x in S.state().get("surfaces", [])):
                break
            time.sleep(0.2)
        time.sleep(1.5)
        st = S.state()
        wx, wy, ww, wh = map(float, re.findall(r"-?[\d.]+", st["window_frame"])[:4])
        # The lab tab splits in two; its left pane holds the lines, right of the 300 pt sidebar.
        x = int(wx + 300 + (ww - 300) / 4)
        y = int(screen_h - (wy + wh) + wh / 2)
        say(f"shell window {st['window_frame']} screen height {screen_h} -> swipe at {x},{y}", log)
        SP.run(["cua", "sb", "screenshot", SP.REF, "-o", os.path.join(out, "shell.png")])
        for i in range(runs):
            swipe(x, y, f"shell-{i + 1}", out, log)
            time.sleep(1)
    finally:
        if shell_started:
            S.app("stop")  # space.py stop: quits the Shell, drops the bridge, frees the lock
            S.lab("down")
        elif SP.gexec("pkill -x ghostty; sleep 0.8", check=False) and ghostty_gone():
            if fresh:
                shutil.rmtree(SP.LOCK, ignore_errors=True)
        else:
            print("Ghostty may still run in the Space; the lock is kept", file=sys.stderr)
        say(f"held {time.time() - t0:.0f}s", log)

    say("", log)
    incomplete = []
    for label in [f"{app}-{i + 1}" for app in ("ghostty", "shell") for i in range(runs)]:
        summary = summarize(os.path.join(out, label + ".tsv"))
        say(f"{label}: {json.dumps(summary)}", log)
        if not summary or summary["moving_frames"] < MIN_MOVING:
            incomplete.append(label)
    if incomplete:
        say(f"INCOMPLETE capture: {', '.join(incomplete)}", log)
    open(os.path.join(out, "summary.txt"), "w").write("\n".join(log) + "\n")
    if incomplete:
        sys.exit(1)


if __name__ == "__main__":
    main()
