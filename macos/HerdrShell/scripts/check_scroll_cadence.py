#!/usr/bin/env python3
"""Per-frame scroll cadence check: in the Cua Space, a Herdr Shell pane moves its rows out
as evenly per display frame as Ghostty.app does for the same trackpad swipe.

  python3 scripts/check_scroll_cadence.py [--out checks/scroll-cadence.txt]
                                          [--attach-bin PATH] [--runs N] [--ghostty]

check_scroll.py holds the row count of each swipe to Ghostty.app. This one holds how the
rows land. The swipes of check_scroll.py's PROFILES are posted to the HID tap
(scroll-ref/inject.swift) over a pane of numbered lines, and scroll-ref/framecap.swift
records every frame the window server composites for the window (ScreenCaptureKit) and
reads its first visible line. Per 1/60 s display frame during the motion:
  max    the most rows moved in one frame
  stalls frames that moved nothing between two frames that moved 2 rows or more
Both must stay within Ghostty.app's numbers for the profile (GHOSTTY_CADENCE, taken with
--ghostty, which runs the same swipes in Ghostty.app 1.3.1 with the Shell's font config).

The herdr server runs inside the guest (scroll-ref/cadence-lab.sh), as it does on a real
Mac: over the ssh bridge to a Studio lab the frames take the bridge's jitter too. Before
the attach-scroll fix (2026-10-06), the server held each wheel report's frame for its
16 ms render cadence; out of phase with the Shell's display, rows landed 0 then 4 to 10
at a time (checks/scroll-cadence-base.txt).
"""
import os
import sys
import time

D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REF = os.path.join(D0, "scripts", "scroll-ref")
sys.path.insert(0, os.path.join(D0, "scripts"))
import space as SP  # noqa: E402
from check_scroll import PROFILES  # noqa: E402

GHOSTTY = "--ghostty" in sys.argv
OUT = (sys.argv[sys.argv.index("--out") + 1] if "--out" in sys.argv
       else os.path.join(D0, "checks", "scroll-cadence-ghostty.txt" if GHOSTTY else "scroll-cadence.txt"))
ATTACH_BIN = (sys.argv[sys.argv.index("--attach-bin") + 1] if "--attach-bin" in sys.argv
              else os.environ.get("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr")))
RUNS = int(sys.argv[sys.argv.index("--runs") + 1]) if "--runs" in sys.argv else 2
G = "/Users/lume/scroll-cadence"
VSYNC = 1000 / 60
# (window app name, pointer x y, text crop x y w h in window points): the first column of
# the pane, narrow, because Vision reads nothing in a tall crop that is mostly blank.
TARGET = {"shell": ("HerdrShell", 600, 400, (300, 36, 100, 640)),
          "ghostty": ("Ghostty", 512, 369, (0, 36, 110, 640))}
# Ghostty.app, three runs per profile with --ghostty, 2026-10-06 (max rows in a frame,
# stalls): the worst run of each.
GHOSTTY_CADENCE = {"slow": (1, 0), "medium": (2, 0), "std": (3, 0), "fast": (5, 0)}
lines, failures = [], []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({detail})" if detail else ""))
    if not ok:
        failures.append(name)


def per_frame(tsv):
    """Rows moved in each display frame from the first move to the last; a gap of k frames
    with no composite holds the rows still for k-1 frames."""
    pts = []
    for l in tsv.splitlines():
        if l.startswith("#"):
            continue
        ms, _, v = l.partition("\t")
        if v.strip().isdigit():
            pts.append((float(ms), int(v)))
    moves = [i for i in range(1, len(pts)) if pts[i][1] != pts[i - 1][1]]
    if not moves:
        return [], len(pts)
    out = []
    for i in range(moves[0], moves[-1] + 1):
        k = max(1, round((pts[i][0] - pts[i - 1][0]) / VSYNC))
        out += [0] * (k - 1) + [abs(pts[i][1] - pts[i - 1][1])]
    return out, len(pts)


def cadence(d):
    stalls = sum(1 for i in range(1, len(d) - 1) if d[i] == 0 and d[i - 1] >= 2 and d[i + 1] >= 2)
    return max(d, default=0), stalls


def g(cmd, timeout=300):
    return SP.gexec(cmd, timeout=timeout).stdout


def main():
    mode = "ghostty" if GHOSTTY else "shell"
    app, x, y, crop = TARGET[mode]
    say(f"HerdrShell scroll cadence check ({mode})  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    owner = SP.owner_name()
    fresh = SP.lock_take(owner, 240)
    try:
        SP.up()
        SP.stop_app()
        SP.gexec(f"mkdir -p {G}")
        for f in ("framecap.swift", "inject.swift", "cadence-lab.sh"):
            SP.push(os.path.join(REF, f), f"{G}/{f}")
        SP.push(os.path.join(D0, "Resources", "keymap.json"), "/Users/lume/scroll/keymap.json")
        SP.push(os.path.abspath(ATTACH_BIN), f"{G}/herdr")
        if not GHOSTTY:
            SP.push(os.path.join(D0, ".build", "release", "HerdrShell"), f"{G}/HerdrShell")
            say(f"attach and server binary: {ATTACH_BIN}")
        say(g(f"cd {G} && swiftc -O framecap.swift -o framecap 2>&1 | grep error; "
              f"swiftc -O inject.swift -o inject 2>&1 | grep error; true").strip())
        target = "ghostty" if GHOSTTY else f"{G}/HerdrShell"
        say("lab: " + g(f"sh {G}/cadence-lab.sh {G}/herdr {target}").strip())
        say("window: " + g(f"{G}/framecap {app} bounds").strip())
        c = " ".join(map(str, crop))
        worst = {}
        for run in range(1, RUNS + 1):
            for name, (dy, steps, momentum, decay) in PROFILES.items():
                tsv = f"/tmp/cadence-{name}-{run}.tsv"
                g(f"cd {G}; ./inject 0 {x} {y} -40 60 0 0.9 8.333 hid >/dev/null; sleep 1; "
                  f"./framecap {app} 3.5 {tsv} {c} > {tsv}.log 2>&1 & sleep 0.8; "
                  f"./inject 0 {x} {y} {dy} {steps} {momentum} {decay} 8.333 hid > /dev/null; wait")
                d, read = per_frame(g(f"cat {tsv}"))
                mx, stalls = cadence(d)
                w = worst.get(name, (0, 0, 0))
                worst[name] = (max(w[0], mx), max(w[1], stalls), max(w[2], sum(d)))
                say(f"{name} run {run}: {sum(d)} rows over {len(d)} frames ({read} frames read), "
                    f"max {mx}, stalls {stalls}: {' '.join(map(str, d))}")
        say()
        for name, (mx, stalls, rows) in worst.items():
            if GHOSTTY:
                say(f"{name}: Ghostty.app worst max {mx}, stalls {stalls}")
                continue
            gmax, gstalls = GHOSTTY_CADENCE[name]
            check(f"{name} swipe: no frame moves more rows than Ghostty.app's", mx <= gmax,
                  f"Shell {mx}, Ghostty {gmax}")
            check(f"{name} swipe: no stalled frame mid-swipe beyond Ghostty.app's", stalls <= gstalls,
                  f"Shell {stalls}, Ghostty {gstalls}")
            check(f"{name} swipe: the motion was read", rows > 0, f"{rows} rows")
        SP.gexec(f"sh {G}/cadence-lab.sh {G}/herdr stop", check=False)
    finally:
        if fresh:
            SP.shutil.rmtree(SP.LOCK, ignore_errors=True)
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
