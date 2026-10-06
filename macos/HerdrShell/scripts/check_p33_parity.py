#!/usr/bin/env python3
"""Compare the native tree with independently transcribed Rust semantic goldens.

Owns section order, labels, folding, nesting, goals and footer metadata. A flat
sidebar or incorrect overlay grouping fails here; UI lifecycle belongs to P33's
Space scenario. Expected files must never be recorded from this driver.
"""
import difflib
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "scripts/fixtures/p33"
BUILD = pathlib.Path.home() / ".cache/herdr-build/p33"
BUILD.mkdir(parents=True, exist_ok=True)
DRIVER = BUILD / "p33_dump"
subprocess.run(["swiftc", str(ROOT / "Sources/HerdrShell/SpacesTree.swift"),
                str(ROOT / "scripts/p33_dump.swift"), "-o", str(DRIVER)], check=True)
failures = []
for fixture in sorted(FIXTURES.glob("*.json")):
    actual = subprocess.check_output([str(DRIVER), str(fixture)], text=True)
    expected = fixture.with_suffix(".expected.txt").read_text()
    if actual != expected:
        failures.append(fixture.stem)
        print("".join(difflib.unified_diff(expected.splitlines(True), actual.splitlines(True),
                                          fromfile=fixture.stem + " expected", tofile="actual")))
    else:
        print("PASS " + fixture.stem)

fixture = FIXTURES / "sectioned.json"
base = subprocess.check_output([str(DRIVER), str(fixture)], text=True).splitlines()
folded = subprocess.check_output([str(DRIVER), str(fixture), "section:ws_1:IMPLEMENTING"], text=True).splitlines()
start = next(i for i, row in enumerate(base) if row.startswith("section|section:ws_1:IMPLEMENTING|"))
end = next(i for i in range(start + 1, len(base)) if base[i].startswith("group|"))
if folded[:start] != base[:start] or folded[start + 1:] != base[end:]:
    failures.append("fold locality")
else:
    print("PASS fold locality (only IMPLEMENTING changes)")
restored = subprocess.check_output([str(DRIVER), str(fixture), "section:ws_1:IMPLEMENTING", "section:ws_1:IMPLEMENTING"], text=True).splitlines()
if restored != base:
    failures.append("fold round-trip")
else:
    print("PASS fold round-trip")
if failures:
    raise SystemExit("FAIL: " + ", ".join(failures))
print("PASS P33 parity")
