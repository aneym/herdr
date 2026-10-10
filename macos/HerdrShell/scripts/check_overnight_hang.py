#!/usr/bin/env python3
"""Herdr Shell keeps answering through a night of display sleep and wake under load.

  python3 scripts/check_overnight_hang.py [--source path/to/GhosttyConfigMerge.swift]
                                          [--lines N] [--cycles N] [--idle SECONDS]

The overnight hang (stackshot 2026-10-07 08:48; unified log 2026-10-10 04:31 ET): a
libghostty renderer thread blocked for hours in CVDisplayLink::stop() after the display
slept, stopped draining its mailbox, and the main thread then blocked behind it in
ghostty_surface_set_content_scale when the display woke and the backing scale changed.
The fix is the shell's config merge forcing `window-vsync = false`, so no surface ever
creates a CVDisplayLink, whatever Alex's own Ghostty config says.

The driver links the real libghostty and the real merge (GhosttyConfigMerge.swift), opens
one off-screen surface fed by a subprocess printing --lines lines, then on the main thread
replays --cycles display sleep/wake rounds (occlusion, content scale, display id, size),
idles --idle seconds and makes one last wake. It must report vsync off, no CoreVideo
display-link thread at any point, every main-thread round bounded and the final wake
bounded. Pointing --source at the pre-fix merge makes the first two fail.

It cannot put the real display to sleep (that would sleep Alex's screen), so it proves
the hung component is gone, not that CoreVideo would have hung.
"""
import json
import os
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]


def arg(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


SRC = pathlib.Path(arg("--source", ROOT / "Sources/HerdrShell/GhosttyConfigMerge.swift")).resolve()
LINES, CYCLES, IDLE = int(arg("--lines", 400000)), int(arg("--cycles", 2000)), arg("--idle", "20")
VENDOR = pathlib.Path(os.environ.get(
    "GHOSTTY_LIB", "/Volumes/StudioExt/repos/herdr-shell-spikes/vendor/ghostty/macos/GhosttyKit.xcframework/macos-arm64"))
if (ROOT / "Vendor/GhosttyKit/libghostty-internal.a").exists():
    VENDOR = ROOT / "Vendor/GhosttyKit"
BUILD = pathlib.Path.home() / ".cache/herdr-build/overnight-hang"
BUILD.mkdir(parents=True, exist_ok=True)
DRIVER = BUILD / "driver"
FRAMEWORKS = ["AppKit", "Carbon", "CoreGraphics", "CoreText", "CoreVideo", "IOSurface", "Metal",
              "QuartzCore", "UniformTypeIdentifiers"]
cmd = ["nice", "-n", "10", "swiftc", "-O", "-import-objc-header", str(ROOT / "Sources/GhosttyKit/include/ghostty.h"),
       str(SRC), str(ROOT / "scripts/overnight_hang/main.swift"), "-L", str(VENDOR), "-lghostty-internal", "-lc++",
       "-o", str(DRIVER)]
for f in FRAMEWORKS:
    cmd += ["-framework", f]
subprocess.run(cmd, check=True)

# Alex's config may turn vsync back on; the merge must still win.
with tempfile.NamedTemporaryFile("w", suffix=".conf", delete=False) as user:
    user.write("window-vsync = true\nfont-size = 14\n")
try:
    run = subprocess.run([str(DRIVER), str(ROOT / "Resources/ghostty.conf"), user.name, str(LINES), str(CYCLES), IDLE],
                         capture_output=True, text=True, timeout=240, cwd=tempfile.gettempdir())
finally:
    os.unlink(user.name)
out = [l for l in run.stdout.splitlines() if l.startswith("{")]
if run.returncode != 0 or not out:
    print(run.stdout[-2000:], run.stderr[-2000:], sep="\n")
    sys.exit(f"FAIL driver exited {run.returncode}")
r = json.loads(out[-1])
print(json.dumps(r))

failures = []
if r["vsync"]:
    failures.append("window-vsync is on in the merged config: surfaces create a CVDisplayLink")
if r["display_link_threads"] != 0:
    failures.append(f"{r['display_link_threads']} CVDisplayLink thread(s) ran in the process")
if r["max_main_call_ms"] > 250:
    failures.append(f"a main-thread sleep/wake round took {r['max_main_call_ms']} ms (bound 250)")
if r["wake_ms"] > 1000:
    failures.append(f"the wake after idle took {r['wake_ms']} ms (bound 1000)")
if r["max_heartbeat_gap_ms"] > 2000:
    failures.append(f"the main run loop stalled {r['max_heartbeat_gap_ms']} ms (macOS marks Not Responding at 2000)")
for f in failures:
    print("FAIL", f)
if failures:
    sys.exit(1)
print(f"PASS overnight hang: {r['lines']} lines, {r['cycles']} sleep/wake rounds, {IDLE}s idle; "
      f"no display link, worst main-thread round {r['max_main_call_ms']} ms, wake {r['wake_ms']} ms")
