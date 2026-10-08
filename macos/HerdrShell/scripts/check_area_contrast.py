#!/usr/bin/env python3
"""Compile and run the area-dot contrast cases against the shell's generated palettes."""
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "Sources/HerdrShell"
BUILD = pathlib.Path.home() / ".cache/herdr-build/area-contrast"
BUILD.mkdir(parents=True, exist_ok=True)
DRIVER = BUILD / "area_contrast"
subprocess.run([
    "swiftc", "-parse-as-library", str(SRC / "Theme.swift"),
    str(SRC / "ShellTokens.swift"), str(SRC / "AreaContrast.swift"),
    str(ROOT / "scripts/area_contrast.swift"), "-o", str(DRIVER),
], check=True)
subprocess.run([str(DRIVER)], check=True)
