#!/usr/bin/env python3
"""Golden tables exercise pure desk transition edge cases via the Swift interpreter."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
def item(id):
    return dict(id=id, kind="url", ref="https://example.com/" + id, title=id,
                mime="text/html", opened_by="user", opened_at_ms=1)
def desk(ids, front):
    return dict(items=[item(i) for i in ids], front=front)
cases = []
def case(name, previous, current, old, active, expected_landed, expected_active, tab="a"):
    cases.append((dict(name=name, previous=previous, current=current, previousFront=old,
                       active=active, tab=tab), dict(name=name, landed=expected_landed, active=expected_active)))
case("first load opens nothing", None, {"a": desk(["d1"], "d1")}, None, None, [], "d1")
case("new item on another tab", {"a": ["d1"]}, {"a": desk(["d1"], "d1"), "b": desk(["d2"], "d2")}, "d1", "d1", ["b"], "d1")
case("closed item opens nothing", {"a": ["d1", "d2"]}, {"a": desk(["d1"], "d1")}, "d1", "d1", [], "d1")
case("server front changed", {"a": ["d1", "d2"]}, {"a": desk(["d1", "d2"], "d2")}, "d1", "d1", [], "d2")
case("user active kept", {"a": ["d1", "d2"]}, {"a": desk(["d1", "d2"], "d1")}, "d1", "d2", [], "d2")
case("removed active follows front", {"a": ["d1", "d2"]}, {"a": desk(["d1"], "d1")}, "d1", "d2", [], "d1")
case("empty desk", {"a": ["d1"]}, {"a": desk([], None)}, "d1", "d1", [], None)
case("remote first snapshot opens nothing", {"a": ["d1"]},
     {"a": desk(["d1"], "d1"), "ax42/w1:t2": desk(["r1"], "r1")},
     None, None, [], "r1", tab="ax42/w1:t2")
case("new remote item lands", {"a": ["d1"], "ax42/w1:t2": ["r1"]},
     {"a": desk(["d1"], "d1"), "ax42/w1:t2": desk(["r1", "r2"], "r2")},
     "r1", "r1", ["ax42/w1:t2"], "r2", tab="ax42/w1:t2")
source = (root / "Sources/HerdrShell/DeskModel.swift").read_text() + (root / "scripts/desk_model_dump.swift").read_text()
# Source is supplied to `swift -`; table data travels on stdin through a file
# argument so the interpreter does not consume both source and data from stdin.
import tempfile
with tempfile.TemporaryDirectory() as directory:
    table = Path(directory) / "cases.json"
    table.write_text(json.dumps([c for c, _ in cases]))
    result = subprocess.run(["swift", "-", str(table)], input=source, text=True, capture_output=True, cwd=root, timeout=240)
if result.returncode:
    raise SystemExit(result.stderr)
actual = [json.loads(line) for line in result.stdout.splitlines()]
assert actual == [expected for _, expected in cases], (actual, cases)
summary = f"DESK MODEL: PASS ({len(cases)} transition cases, Swift interpreter)\n" + "\n".join("PASS " + c[0]["name"] for c in cases) + "\n"
(root / "checks").mkdir(exist_ok=True)
(root / "checks/DESK-MODEL.txt").write_text(summary)
print(summary, end="")
