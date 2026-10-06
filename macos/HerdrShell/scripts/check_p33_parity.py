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
def base_rows(path):
    return subprocess.check_output([str(DRIVER), str(path)], text=True).splitlines()
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
# Lanes fold by default (C46): one chevron click must open a lane that is in
# neither set, and a second click folds it again.
import json
fresh = json.loads((FIXTURES / "base.json").read_text())
fresh["chrome"]["collapsedTabs"] = ["orch"]  # lane-a sits in neither fold set
fixture = BUILD / "base-fresh.json"
fixture.write_text(json.dumps(fresh))
opened = subprocess.check_output([str(DRIVER), str(fixture), "tab:lane-a"], text=True).splitlines()
want = ["tab|tab:lane-a|1|open|", "tab|tab:wf-a|2|", "tab|tab:wf-b|2|"]
at = [next((i for i, row in enumerate(opened) if row.startswith(prefix)), -1) for prefix in want]
refolded = subprocess.check_output([str(DRIVER), str(fixture), "tab:lane-a", "tab:lane-a"], text=True).splitlines()
if -1 in at or at != sorted(at) or at[1] != at[0] + 1 or refolded != base_rows(fixture):
    failures.append("default-folded lane toggle")
else:
    print("PASS default-folded lane opens on one click and folds on the next")
focused = dict(fresh, input=dict(fresh["input"], focusedTab="wf-a"))
fixture = BUILD / "base-focused.json"
fixture.write_text(json.dumps(focused))
held = base_rows(fixture)
clicked = subprocess.check_output([str(DRIVER), str(fixture), "tab:lane-a"], text=True).splitlines()
if not any(r.startswith("tab|tab:lane-a|1|open|") for r in held) or not any(r.startswith("tab|tab:lane-a|1|closed|") for r in clicked):
    failures.append("focus-held lane folds on one click")
else:
    print("PASS a lane held open by focus folds on one click")
# Space groups come from data (areas.json space_groups, merged into the overlay), never workspace-id code.
# As Rust space_groups_from_areas_file_split_the_sidebar_by_label_or_id.
fresh = json.loads((FIXTURES / "base.json").read_text())
space = fresh["input"]["spaces"][0]
other = dict(space, id="ws_of", name="open factory")
fresh["input"]["spaces"].insert(0, other)
fresh["input"]["tabs"].append(dict(fresh["input"]["tabs"][0], id="of-lane", space="ws_of", label="of-lane", focused=False))
fixture = BUILD / "spaces-split.json"
fixture.write_text(json.dumps(fresh))
plain = base_rows(fixture)
fresh["overlay"]["space_groups"] = [{"name": "Rails", "spaces": [space["name"].upper()]},
                                    {"name": "Open Factory", "spaces": ["ws_of"]}]
fixture.write_text(json.dumps(fresh))
split = base_rows(fixture)
def at(prefix):
    return next((i for i, r in enumerate(split) if r.startswith(prefix)), None)
order = [at("title|spacegroup:Rails|"), at("space|space:" + space["id"] + "|"),
         at("title|spacegroup:Open Factory|"), at("space|space:ws_of|")]
if (any(r.startswith("title|spacegroup:") for r in plain) or None in order or order != sorted(order)):
    failures.append("spaces split")
    print("\n".join(split))
else:
    print("PASS spaces split follows space_groups data")
# A pinned lane rolls up its live children as its header in the space does (H0 6b): the lane
# below is idle with one live run, so its space row reads working and so must its pinned row.
pinned = json.loads((FIXTURES / "live-child-working.json").read_text())
pinned["input"]["tabs"][0]["pinIndex"] = 0
fixture = BUILD / "pinned-live-child.json"
fixture.write_text(json.dumps(pinned))
rows = base_rows(fixture)
def mark(prefix):
    row = next((r.split("|") for r in rows if r.startswith(prefix)), None)
    return row and (row[4], row[5])
if mark("tab|tab:lane|") != ("●", "working") or mark("tab|pinned:lane|") != mark("tab|tab:lane|"):
    failures.append("pinned lane rollup")
    print("\n".join(rows))
else:
    print("PASS a pinned lane with live children reads working, as its space row does")
if failures:
    raise SystemExit("FAIL: " + ", ".join(failures))
print("PASS P33 parity")
