#!/usr/bin/env python3
"""Golden click/refusal policy table compiled from production Swift, no app.

Protects resolve-bound click/refusal policy and server full-target precedence.
The attach stream has no displayed-frame metadata; the server resolve guards
are the accepted binding, not a proof of displayed-frame identity.
The lead owns real gesture/selection/mouse-reporting proofs in Space.
"""
import os
import pathlib
import subprocess
import tempfile
import contextlib
import io

ROOT = pathlib.Path(__file__).resolve().parents[1]
output = io.StringIO()
original_print = print
def print(*args, **kwargs):
    original_print(*args, **kwargs)
    with contextlib.redirect_stdout(output):
        original_print(*args, **kwargs)
CASES = [
    ('clipped prefix hover uses server URL', 'TerminalLinkDecision.hoverTarget(serverURL: "https://example.com/full", fragment: "com/full")', 'https://example.com/full'),
    ('older server hover uses fragment', 'TerminalLinkDecision.hoverTarget(serverURL: nil, fragment: "https://example.com/full")', 'https://example.com/full'),
    ('successful activation without URL uses resolve', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/a", activated: nil, handled: false) ?? "none"', 'https://example.com/a'),
    ('click before hover resolves', 'TerminalLinkDecision.needsResolution(cachedURL: nil)', 'true'),
    ('click with completed hover', 'TerminalLinkDecision.needsResolution(cachedURL: "https://example.com/a")', 'false'),
    ('refused activation opens nothing', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/a", activated: nil, handled: false, succeeded: false) ?? "none"', 'none'),
    ('activation URL is authoritative', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/a", activated: "https://example.com/b", handled: false) ?? "none"', 'https://example.com/b'),
    ('uncached activation supplies full URL', 'TerminalLinkDecision.openTarget(resolved: nil, activated: "https://example.com/full", handled: false) ?? "none"', 'https://example.com/full'),
    ('plugin handled opens nothing twice', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/a", activated: "https://example.com/a", handled: true) ?? "none"', 'none'),
    ('no resolved target and refusal is native miss', 'TerminalLinkDecision.openTarget(resolved: nil, activated: nil, handled: false) ?? "none"', 'none'),
]
with tempfile.TemporaryDirectory(prefix='terminal-links-', dir=os.environ.get('TMPDIR')) as scratch:
    scratch = pathlib.Path(scratch)
    source = scratch / 'main.swift'
    source.write_text('\n'.join(f'print({expression})' for _, expression, _ in CASES) + '\n')
    # Interpret production policy to avoid the Studio fresh-executable launch issue.
    source.write_text((ROOT / "Sources/HerdrShell/TerminalLinkDecision.swift").read_text() + source.read_text())
    rows = subprocess.check_output(["swift", str(source)], text=True, timeout=120).splitlines()
failures = 0
for index, (name, _, expected) in enumerate(CASES):
    actual = rows[index] if index < len(rows) else '<missing>'
    ok = actual == expected
    failures += not ok
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f' (got {actual})' if not ok else ''))
if len(rows) != len(CASES):
    failures += 1
# Wiring checks supplement the compiled golden policy; real gestures remain manual.
links = (ROOT / "Sources/HerdrShell/TerminalLinks.swift").read_text()
client = (ROOT / "Sources/HerdrShell/HerdrClient.swift").read_text()
surface = (ROOT / "Sources/HerdrShell/SurfaceView.swift").read_text()
runtime = (ROOT / "Sources/HerdrShell/GhosttyRuntime.swift").read_text()
for name, ok in [
    ("activation sends cached resolve guards", "resolution: found" in links and "? resolution : nil" in links
     and 'params["content_revision"] = revision' in client and 'params["offset_from_bottom"] = offset' in client),
    ("refusal clears cache and suppresses native replay", "guard let answer else {\n                    self?.clear()\n                    completion(true)" in links and "resolution = nil" in links),
    ("Shift comes from triggering event including native replay", "linkEventShift = event.modifierFlags.contains(.shift)" in surface
     and "shift = view.linkEventShift" in runtime and "NSEvent.modifierFlags" not in runtime),
]:
    failures += not ok
    print(f"[{'PASS' if ok else 'FAIL'}] {name}")
print(f"{len(CASES) + 3 - failures} passed; {failures} failed")
print("[UNVERIFIED] real AppKit gestures and displayed-frame identity; guards bind to server resolution")
(ROOT / "checks").mkdir(exist_ok=True)
(ROOT / "checks/TERMINAL-LINKS.txt").write_text(output.getvalue())
raise SystemExit(bool(failures))
