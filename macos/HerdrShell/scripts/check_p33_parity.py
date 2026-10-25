#!/usr/bin/env python3
"""Compare the native tree with independently transcribed Rust semantic goldens.

Owns section order, labels, folding, nesting, goals and footer metadata. A flat
sidebar or incorrect overlay grouping fails here; UI lifecycle belongs to P33's
Space scenario. Expected files must never be recorded from this driver.
"""
import difflib
import pathlib
import tempfile
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "scripts/fixtures/p33"
_scratch = tempfile.TemporaryDirectory(prefix="herdr-p33-")
BUILD = pathlib.Path(_scratch.name)
DRIVER = BUILD / "p33_dump"
subprocess.run(["swiftc", *(str(ROOT / "Sources/HerdrShell" / name) for name in ("SpacesModels.swift", "HomeRows.swift", "SpacesTree.swift", "SpaceScope.swift", "Automations.swift")),
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
folded = subprocess.check_output([str(DRIVER), str(fixture), "section:ws_1:LANES"], text=True).splitlines()
start = next(i for i, row in enumerate(base) if row.startswith("section|section:ws_1:LANES|"))
end = next(i for i in range(start + 1, len(base)) if base[i].startswith("group|"))
if folded[:start] != base[:start] or folded[start + 1:] != base[end:]:
    failures.append("fold locality")
else:
    print("PASS fold locality (only LANES changes)")
restored = subprocess.check_output([str(DRIVER), str(fixture), "section:ws_1:LANES", "section:ws_1:LANES"], text=True).splitlines()
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
# herdr's work fact wins over the overlay's busy copy (2026-10-06, w5P:t7 "submissions"): the chat
# sat idle at its prompt (work "done") while the overlay still listed a finished teammate as a live
# agent: run and the tab busy. Space row and pinned row must read it finished and its run row is
# gone (ASKS-11: a finished run's row drops); with work "working" the same overlay reads working.
# As Rust pinned_and_lane_rows_follow_the_endpoint_chat_status_over_a_stale_busy_overlay.
for work, want_tone, want_run in [("done", "done", None), ("working", "working", "◐")]:
    stale = json.loads((FIXTURES / "live-child-working.json").read_text())
    # The agents report the opposite of work, so the rows can only follow the work fact.
    other = "working" if work == "done" else "done"
    stale["input"]["tabs"][0].update(pinIndex=0, status=other, work=work,
                                     agents=[{"status": other}])
    stale["overlay"]["tabs"]["lane"].update(busy=True, runs=[
        {"id": "agent:agent-atrial-chase-lists-c98d7b9ae81a05d0", "name": "trial-chase-lists", "phase": "opus-seat"}])
    stale["chrome"]["expandedTabs"] = ["lane"]
    fixture = BUILD / f"work-{work}.json"
    fixture.write_text(json.dumps(stale))
    rows = base_rows(fixture)
    def cell(prefix, field):
        row = next((r.split("|") for r in rows if r.startswith(prefix)), None)
        return row and row[field]
    if (cell("tab|tab:lane|", 5) != want_tone or cell("run|run:agent:", 4) != want_run):
        failures.append(f"work fact {work}")
        print("\n".join(rows))
    else:
        print(f"PASS a {work} chat reads {want_tone} in its space row and run row over a busy overlay")
# ASKS-11: a run's row drops when the run ends, so no workflow row outlives its run with a stale
# age; a running run keeps its row. As Rust registered_run_progress_renders_under_lane_and_finished_runs_drop.
runs = json.loads((FIXTURES / "live-child-working.json").read_text())
runs["overlay"]["tabs"]["lane"]["runs"] = [
    {"id": "wf_live", "name": "embed 5", "phase": "Build 2/3", "started": 1000 - 600},
    {"id": "wf_done", "name": "embed 2", "phase": "Build 3/3", "started": 1000 - 32400, "done": True},
    {"id": "wf_failed", "name": "embed 7", "started": 1000 - 21600, "done": True, "attention": "act"}]
runs["chrome"]["expandedTabs"] = ["lane"]
fixture = BUILD / "finished-runs.json"
fixture.write_text(json.dumps(runs))
rows = base_rows(fixture)
run_rows = [r.split("|")[1] for r in rows if r.startswith("run|")]
lane = next((r.split("|") for r in rows if r.startswith("tab|tab:lane|")), None)
if run_rows != ["run:wf_live"] or not lane or lane[8] != "act":
    failures.append("finished runs drop")
    print("\n".join(rows))
else:
    print("PASS a finished run's row drops, a running run's row stays, a failure keeps the lane's !")
if failures:
    raise SystemExit("FAIL: " + ", ".join(failures))
print("PASS P33 parity")
