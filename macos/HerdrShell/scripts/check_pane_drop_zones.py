#!/usr/bin/env python3
"""Mac drop zones match the shared fixture (pane drag slice S6; owner-written, do not edit).

  python3 macos/HerdrShell/scripts/check_pane_drop_zones.py

Pure: compiles Sources/HerdrShell/PaneDrop.swift with scripts/pane_drop_dump.swift and runs
the driver over shell/fixtures/pane-drop-zones.json, the fixture the TUI (pane_drop.rs) and
Windows (paneDrop.ts) also run. No app, no server, no Space.

Checks, for every fixture case:
  - the zone (kind, target pane, side) equals the fixture's;
  - the estimate rect equals the fixture's (to 1e-3) whenever the fixture has one;
  - the same at scale 8 (lengths in points, as the Mac runs it): the zone is identical and
    the estimate is within half a cell (8 * 0.5 pt) of 8 x the fixture's.
Writes macos/HerdrShell/checks/PANE-DROP-ZONES.txt. A driver that hangs at launch for 60 s
is reported BLOCKED (exit 2), not retried.
"""
import json
import os
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
FIXTURE = REPO / "shell/fixtures/pane-drop-zones.json"
SCALE = 8


def expected_zone(case):
    zone = case.get("zone")
    if zone is None:
        return "none"
    kind = zone["kind"]
    if kind == "centre":
        return f"centre:{zone['target']}"
    if kind == "pane_edge":
        return f"pane_edge:{zone['target']}:{zone['side']}"
    if kind == "tab_edge":
        return f"tab_edge:{zone['side']}"
    raise SystemExit(f"fixture case {case['name']!r}: unknown zone kind {kind!r}")


def compare(fixture, rows, scale, check):
    """rows: driver lines for one scale. Every case must appear once, in fixture order."""
    cases = fixture["cases"]
    check(f"x{scale}: one driver line per fixture case", len(rows) == len(cases), f"{len(rows)} lines for {len(cases)} cases")
    for index, case in enumerate(cases):
        label = f"x{scale} #{index} {case['name']}"
        row = rows[index] if index < len(rows) else ""
        parts = row.split("|", 3)
        if len(parts) != 4 or parts[0] != str(index):
            check(label, False, f"malformed or out-of-order line {row!r}")
            continue
        _, zone, estimate, _ = parts
        want_zone = expected_zone(case)
        if zone != want_zone:
            check(label, False, f"zone {zone} want {want_zone}")
            continue
        want_estimate = case.get("estimate")
        if want_zone == "none":
            check(label, estimate == "-", f"estimate {estimate} for no zone")
            continue
        if want_estimate is None:
            check(label, estimate != "-", "no estimate for a zone")
            continue
        try:
            got = [float(v) for v in estimate.split(",")]
        except ValueError:
            check(label, False, f"estimate {estimate!r} is not x,y,w,h")
            continue
        want = [v * scale for v in want_estimate]
        tolerance = 1e-3 if scale == 1 else scale * 0.5 + 1e-3
        ok = len(got) == 4 and all(abs(g - w) <= tolerance for g, w in zip(got, want))
        check(label, ok, f"estimate {got} want {want} (+-{tolerance:g})")


def main():
    lines, failures = [], []

    def check(name, ok, detail=""):
        line = f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" ({detail})" if detail and not ok else "")
        lines.append(line)
        if not ok:
            failures.append(name)

    fixture = json.loads(FIXTURE.read_text())
    blocked = None
    with tempfile.TemporaryDirectory(prefix="herdr-pane-drop-", dir=os.environ.get("TMPDIR")) as scratch:
        driver = pathlib.Path(scratch) / "pane_drop_dump"
        build = subprocess.run(["swiftc", str(ROOT / "Sources/HerdrShell/PaneDrop.swift"),
                                str(ROOT / "scripts/pane_drop_dump.swift"), "-o", str(driver)],
                               capture_output=True, text=True, timeout=300)
        if build.returncode != 0:
            check("PaneDrop.swift compiles with the dump driver alone", False, build.stderr.strip()[-2000:])
        else:
            check("PaneDrop.swift compiles with the dump driver alone", True)
            for scale in (1, SCALE):
                try:
                    out = subprocess.run([str(driver), str(FIXTURE), "--scale", str(scale)],
                                         capture_output=True, text=True, timeout=60)
                except subprocess.TimeoutExpired:
                    blocked = "compiled driver hung at launch over 60 seconds; not retried"
                    break
                if out.returncode != 0:
                    check(f"x{scale}: driver exits 0", False, out.stderr.strip()[-2000:])
                    continue
                compare(fixture, out.stdout.splitlines(), scale, check)
    if blocked:
        lines.append(f"[BLOCKED] {blocked}")
    passed = sum(line.startswith("[PASS]") for line in lines)
    lines.append(f"{passed} passed; {len(failures)} failed" + ("; blocked" if blocked else ""))
    (ROOT / "checks").mkdir(exist_ok=True)
    (ROOT / "checks/PANE-DROP-ZONES.txt").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    if blocked:
        sys.exit(2)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
