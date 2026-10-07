#!/usr/bin/env python3
"""Golden click/refusal policy table compiled from production Swift, no app.

Protects independent-click resolution and resolved-target refusal fallback; the
existing route check does not exercise either. Frame invalidation cannot be
proved until the ANSI attach stream exposes displayed revision/offset metadata.
The lead owns real gesture/selection/mouse-reporting proofs in Space.
"""
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
CASES = [
    ('click before hover resolves', 'TerminalLinkDecision.needsResolution(cachedURL: nil)', 'true'),
    ('click with completed hover', 'TerminalLinkDecision.needsResolution(cachedURL: "https://example.com/a")', 'false'),
    ('refused activation retains resolved URL', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/a", activated: nil, handled: false) ?? "none"', 'https://example.com/a'),
    ('activation cannot replace resolved URL', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/a", activated: "https://example.com/b", handled: false) ?? "none"', 'https://example.com/a'),
    ('activation continuing a viewport-clipped URL supplies the full URL', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/abcdefghijklmnopqrst", activated: "https://example.com/abcdefghijklmnopqrstuv", handled: false) ?? "none"', 'https://example.com/abcdefghijklmnopqrstuv'),
    ('no-hover click with refused activation opens the URL the click resolved', 'TerminalLinkDecision.openTarget(resolved: TerminalLinkDecision.resolvedTarget(cached: nil, clicked: "https://example.com/wrapped"), activated: nil, handled: false) ?? "none"', 'https://example.com/wrapped'),
    ('a completed hover still beats the click resolution', 'TerminalLinkDecision.resolvedTarget(cached: "https://example.com/a", clicked: "https://example.com/b") ?? "none"', 'https://example.com/a'),
    ('uncached activation supplies full URL', 'TerminalLinkDecision.openTarget(resolved: nil, activated: "https://example.com/full", handled: false) ?? "none"', 'https://example.com/full'),
    ('plugin handled opens nothing twice', 'TerminalLinkDecision.openTarget(resolved: "https://example.com/a", activated: "https://example.com/a", handled: true) ?? "none"', 'none'),
    ('no resolved target and refusal is native miss', 'TerminalLinkDecision.openTarget(resolved: nil, activated: nil, handled: false) ?? "none"', 'none'),
]
with tempfile.TemporaryDirectory(prefix='terminal-links-', dir=os.environ.get('TMPDIR')) as scratch:
    scratch = pathlib.Path(scratch)
    source = scratch / 'main.swift'
    source.write_text('\n'.join(f'print({expression})' for _, expression, _ in CASES) + '\n')
    driver = scratch / 'terminal-links'
    subprocess.run(['swiftc', str(ROOT / 'Sources/HerdrShell/TerminalLinkDecision.swift'), str(source), '-o', str(driver)], check=True, timeout=120)
    try:
        rows = subprocess.check_output([str(driver)], text=True, timeout=60).splitlines()
    except subprocess.TimeoutExpired:
        print('[BLOCKED] compiled driver hung at launch over 60 seconds; not retried')
        raise SystemExit(1)
failures = 0
for index, (name, _, expected) in enumerate(CASES):
    actual = rows[index] if index < len(rows) else '<missing>'
    ok = actual == expected
    failures += not ok
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f' (got {actual})' if not ok else ''))
if len(rows) != len(CASES):
    failures += 1
print(f'{len(CASES) - failures} passed; {failures} failed')
print('[UNVERIFIED] displayed-frame revision/offset invalidation: attach transport lacks metadata')
raise SystemExit(bool(failures))
