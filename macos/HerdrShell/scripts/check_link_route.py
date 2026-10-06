#!/usr/bin/env python3
"""Golden URL/Shift policy table compiled from production Swift; no app or server.

Run python3 scripts/check_link_route.py. Writes checks/LINK-ROUTE.txt.
A driver launch exceeding 60 seconds is a blocker, not retried.
"""
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
lines = []
failures = 0
with tempfile.TemporaryDirectory(prefix="herdr-link-route-", dir=os.environ.get("TMPDIR")) as scratch:
    driver = pathlib.Path(scratch) / "link_route_dump"
    subprocess.run(["swiftc", str(ROOT / "Sources/HerdrShell/LinkRoute.swift"),
                    str(ROOT / "scripts/link_route_dump.swift"), "-o", str(driver)],
                   check=True, timeout=120)
    try:
        rows = subprocess.check_output([str(driver)], text=True, timeout=60).splitlines()
    except subprocess.TimeoutExpired:
        lines.append("[BLOCKED] compiled driver hung at launch over 60 seconds; not retried")
        rows = []
        failures += 1
    expected = [f"{scheme}|{shift}|{route}" for scheme, plain, shifted in
                [("http", "desk", "external"), ("https", "desk", "external"),
                 ("file", "desk", "external"), ("mailto", "external", "external"),
                 ("javascript", "ignore", "ignore"), ("ftp", "ignore", "ignore")]
                for shift, route in [("false", plain), ("true", shifted)]]
    if rows:
        for index, want in enumerate(expected):
            got = rows[index] if index < len(rows) else "missing"
            ok = got == want
            failures += not ok
            lines.append(f"[{'PASS' if ok else 'FAIL'}] {want}" + (f" (got {got})" if not ok else ""))
        if len(rows) != len(expected):
            failures += 1
            lines.append(f"[FAIL] expected 12 rows, got {len(rows)}")
    elif not failures:
        failures += 1
        lines.append("[FAIL] driver produced no policy rows")
lines.append(f"{sum(line.startswith('[PASS]') for line in lines)} passed; {failures} failed/blocked")
(ROOT / "checks").mkdir(exist_ok=True)
(ROOT / "checks/LINK-ROUTE.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))
raise SystemExit(1 if failures else 0)
