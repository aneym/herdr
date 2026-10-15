#!/usr/bin/env python3
import json
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
CASES = [
    ("first connect", dict(loaded=False, error=None, pending=0, waited=1), "Connecting to Herdr…", True),
    ("server not up yet", dict(loaded=False, error="herdr events: connect failed", pending=0, waited=2),
     "Waiting for the Herdr server to start", True),
    ("server slow to start shows the wait", dict(loaded=False, error="connect failed", pending=0, waited=95),
     "Waiting for the Herdr server to start · 95 s", True),
    ("server lost after load", dict(loaded=True, error="herdr events: closed", pending=7, waited=300),
     "Reconnecting to Herdr…", False),
    ("agents restoring", dict(loaded=True, error=None, pending=118, waited=40),
     "Restoring 118 agents · pinned first, open a chat to start it now", False),
    ("one agent restoring", dict(loaded=True, error=None, pending=1, waited=40),
     "Restoring 1 agent · pinned first, open a chat to start it now", False),
    ("steady state", dict(loaded=True, error=None, pending=0, waited=900), None, None),
]
with tempfile.TemporaryDirectory(prefix="boot-status-") as tmp:
    binary = pathlib.Path(tmp) / "boot_status_dump"
    subprocess.run(["swiftc", "-parse-as-library", "-module-cache-path", str(pathlib.Path(tmp) / "modules"),
                    str(ROOT / "Sources/HerdrShell/BootStatus.swift"),
                    str(ROOT / "scripts/boot_status_dump.swift"), "-o", str(binary)], check=True)
    outputs = json.loads(subprocess.check_output([str(binary)], input=json.dumps([c for _, c, _, _ in CASES]), text=True))
failed = 0
for (name, _, text, centered), out in zip(CASES, outputs):
    got = (None, None) if out.get("hidden") else (out["text"], out["centered"])
    ok = got == (text, centered)
    failed += not ok
    print(("PASS" if ok else "FAIL") + f" {name}: {got}")
print(f"boot status: {len(CASES) - failed}/{len(CASES)} passed")
sys.exit(1 if failed else 0)
