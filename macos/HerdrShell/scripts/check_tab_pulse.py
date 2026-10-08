#!/usr/bin/env python3
"""tab_pulse: an overlay pulse decodes but never adds a timing line to a sidebar row."""
import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="herdr-tab-pulse-") as scratch:
    driver = pathlib.Path(scratch) / "dump"
    subprocess.run(["swiftc", str(ROOT / "Sources/HerdrShell/SpacesTree.swift"),
                    str(ROOT / "scripts/p33_dump.swift"), "-o", str(driver)], check=True)
    fixture = json.loads((ROOT / "scripts/fixtures/p33/base.json").read_text())
    path = pathlib.Path(scratch) / "fixture.json"
    def rows():
        path.write_text(json.dumps(fixture))
        return subprocess.check_output([str(driver), str(path)], text=True).splitlines()
    baseline = rows()
    for drifting in (False, True):
        fixture["overlay"]["tabs"]["orch"]["pulse"] = {"line": "reply 18s · first act 9s · 140k · inline 0/5", "drifting": drifting}
        assert rows() == baseline
    print("PASS tab_pulse_stays_out_of_rows")
