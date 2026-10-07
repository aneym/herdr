#!/usr/bin/env python3
"""Ghostty.app's report cadence for the check_scroll swipes, in the Cua Space.

  HERDR_SPACE_OWNER=<you> python3 scripts/scroll-ref/ghostty_ref.py OUT_DIR

Takes the Space lock and gives it back. Ghostty.app (guest ~/Applications, the Shell's font
config) runs wheellog.py; inject.swift posts std x3, fast and medium swipes to the HID tap.
Ghostty sends one wheel report per row, so report arrival times bucketed per display frame
(anchored at the first report) are a proxy for the rows each frame moved, not a record of
presented frames: one PTY read stamps every report it holds with one time. Writes
ghostty-wheel.tsv, ghostty-cadence.txt and a screenshot to OUT_DIR. The numbers feed
GHOSTTY_ROWS and GHOSTTY_STD_* in check_scroll.py.
"""
import json
import os
import shutil
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import space as SP  # noqa: E402

G = SP.GUEST_HOME + "/.herdr-space/scrollref"
SWIPES = [("std", 12, 30, 40, 0.92)] * 3 + [("fast", 20, 15, 90, 0.95), ("medium", 6, 30, 40, 0.92)]
FPS_SWIFT = 'import AppKit\nprint(NSScreen.screens.map { $0.maximumFramesPerSecond }.max() ?? 60)\n'


def capture(out):
    SP.up()
    SP.stop_app()
    SP.gexec(f"mkdir -p {G} && rm -f {G}/log.tsv {G}/log.tsv.stop")
    for name in ("wheellog.py", "inject.swift"):
        SP.push(os.path.join(HERE, name), f"{G}/{name}")
    SP.gexec(f"cat > {G}/fps.swift <<'SWIFT'\n{FPS_SWIFT}SWIFT")
    fps = int(SP.gexec(f"cd {G} && swiftc -O inject.swift -o inject && swiftc fps.swift -o fps && ./fps").stdout.split()[-1])
    version = SP.gexec("defaults read ~/Applications/Ghostty.app/Contents/Info.plist CFBundleShortVersionString",
                       check=False).stdout.strip() or "unknown"
    SP.gexec(f"pkill -x ghostty; sleep 0.5; open -na ~/Applications/Ghostty.app --args --font-family='SF Mono' "
             f"--font-size=13.5 --adjust-cell-height=8% --window-position-x=80 --window-position-y=80 "
             f"--window-width=100 --window-height=40 --confirm-close-surface=false "
             f"--command='/usr/bin/python3 {G}/wheellog.py {G}/log.tsv'", check=False)
    time.sleep(4)
    SP.run(["cua", "sb", "screenshot", SP.REF, "-o", os.path.join(out, "ghostty.png")])
    for _, dy, steps, momentum, decay in SWIPES:
        SP.gexec(f"cd {G} && ./inject 0 300 300 {dy} {steps} {momentum} {decay} 8.3333 hid")
        time.sleep(2.5)
    SP.gexec(f"touch {G}/log.tsv.stop; sleep 0.5; pkill -x ghostty", check=False)
    SP.pull(f"{G}/log.tsv", os.path.join(out, "ghostty-wheel.tsv"))
    return fps, version


def cadence(path, fps, version="unknown"):
    times = [float(line.split("\t")[0]) for line in open(path) if line.strip()]
    swipes = [[times[0]]] if times else []
    for a, b in zip(times, times[1:]):
        (swipes[-1].append(b) if b - a < 1000 else swipes.append([b]))
    if len(swipes) != len(SWIPES):
        raise SystemExit(f"{path}: {len(swipes)} swipes logged, {len(SWIPES)} posted; labels would be wrong")
    period = 1000 / fps
    out = [f"Ghostty.app {version} wheel reports per {fps} fps frame (report cadence proxy)"]
    for (name, *_), s in zip(SWIPES, swipes):
        frames = {}
        for t in s:
            frames[int((t - s[0]) // period)] = frames.get(int((t - s[0]) // period), 0) + 1
        rows = list(frames.values())
        gaps = [b - a for a, b in zip(s, s[1:])]
        out.append(f"{name}: rows {len(s)}, frames with rows {len(rows)}, rows per frame "
                   f"{ {n: rows.count(n) for n in sorted(set(rows))} }, max {max(rows)}, "
                   f"longest gap {max(gaps):.1f} ms")
    return "\n".join(out)


def main():
    out = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "ghostty-ref")
    os.makedirs(out, exist_ok=True)
    fresh = SP.lock_take(SP.owner_name(), 240)
    try:
        fps, version = capture(out)
    finally:
        try:
            SP.gexec(f"touch {G}/log.tsv.stop; pkill -x ghostty", check=False)
            SP.stop_app()
        finally:
            if fresh:
                shutil.rmtree(SP.LOCK, ignore_errors=True)
    text = cadence(os.path.join(out, "ghostty-wheel.tsv"), fps, version)
    open(os.path.join(out, "ghostty-cadence.txt"), "w").write(text + "\n")
    print(text)


if __name__ == "__main__":
    main()
