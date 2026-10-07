#!/usr/bin/env python3
"""tab_pulse: decode older overlays and render optional normal/drifting timing lines."""
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
    assert all("pulse:" not in row and "pulse-bold" not in row for row in baseline)
    print("PASS tab_pulse_older_overlay_has_no_extra_row")
    for drifting in (False, True):
        fixture["overlay"]["tabs"]["orch"]["pulse"] = {"line": "reply 18s · first act 9s · 140k · inline 0/5", "drifting": drifting}
        actual = rows()
        assert len(actual) == len(baseline)
        pulse = next(row for row in actual if row.startswith("tab|tab:orch|"))
        assert "|pulse:reply 18s · first act 9s · 140k · inline 0/5" in pulse
        assert ("|pulse-bold" in pulse) == drifting
        assert pulse.split("|")[10] == "orch"
        print("PASS tab_pulse_drifting" if drifting else "PASS tab_pulse_normal")
