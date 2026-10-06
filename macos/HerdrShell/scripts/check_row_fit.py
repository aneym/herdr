#!/usr/bin/env python3
"""Spaces rows fit the sidebar column whatever their text says.

Draws every row of each fixture with the shell's own SpacesRowView at the
sidebar's column width (scripts/row_fit.swift) and fails when one lays out
wider. One such row (a long host summary, title or count) widened the whole
sidebar and pushed every glyph, count and chevron out of line (2026-10-05).
"""
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "Sources/HerdrShell"
BUILD = pathlib.Path.home() / ".cache/herdr-build/row-fit"
BUILD.mkdir(parents=True, exist_ok=True)
DRIVER = BUILD / "row_fit"
subprocess.run(["swiftc", "-parse-as-library", str(SRC / "SpacesTree.swift"), str(SRC / "Theme.swift"), str(SRC / "ShellTokens.swift"),
                str(SRC / "SpacesRowView.swift"), str(ROOT / "scripts/row_fit.swift"), "-o", str(DRIVER)], check=True)
fixtures = [ROOT / "scripts/fixtures/row-fit/overflow.json", ROOT / "scripts/fixtures/p33/alex-0928.json"]
failures = []
for fixture in fixtures:
    for mode in ("dark", "light"):
        run = subprocess.run([str(DRIVER), str(fixture), "--mode", mode], capture_output=True, text=True)
        wide = [line for line in run.stdout.splitlines() if float(line.split()[0]) > 284.5]
        if run.returncode != 0 or wide:
            failures.append(f"{fixture.stem} {mode}")
            print(f"FAIL {fixture.stem} {mode}: " + ("; ".join(wide) or run.stderr.strip()))
        else:
            print(f"PASS {fixture.stem} {mode}: {len(run.stdout.splitlines())} rows fit 284 pt")
if failures:
    raise SystemExit("FAIL: " + ", ".join(failures))
print("PASS row fit")
