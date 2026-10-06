#!/usr/bin/env python3
"""LaneCatalog finds areas.json where the Rust server does and keeps space groups by Rust's rule.

  python3 scripts/check_lane_files.py

Rust reads `space_groups` from the areas.json beside the overlay file
(src/server/headless/factory_overlay.rs: `path.with_file_name("areas.json")`) through serde
(src/factory_overlay.rs apply_areas_file): a missing key clears the groups, a document serde
rejects (null, a non-list, a group with a bad field, half-written JSON) keeps the last parsed
ones, and no file at all leaves the overlay's own. Expectations below are transcribed from that
Rust code, never recorded from this driver (H0 findings 11 and 12, 2026-10-06).
"""
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "Sources/HerdrShell"
BUILD = pathlib.Path.home() / ".cache/herdr-build/lane-files"
BUILD.mkdir(parents=True, exist_ok=True)
DRIVER = BUILD / "lane_files"
subprocess.run(["swiftc", "-parse-as-library", str(SRC / "LaneFiles.swift"), str(SRC / "SpacesTree.swift"),
                str(ROOT / "scripts/lane_files.swift"), "-o", str(DRIVER)], check=True)
failures = []


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" ({detail})" if detail else ""))
    if not ok:
        failures.append(name)


with tempfile.TemporaryDirectory() as tmp:
    tmp = pathlib.Path(tmp)
    home, custom = tmp / "home", tmp / "custom"
    (home / ".agent-rails/herdr").mkdir(parents=True)
    custom.mkdir()
    default_areas, areas = home / ".agent-rails/herdr/areas.json", custom / "areas.json"
    default_areas.write_text('{"space_groups":[{"name":"Default","spaces":["d"]}]}')
    env = {"HOME": str(home), "PATH": "/usr/bin:/bin", "FACTORY_OVERLAY": str(custom / "overlay.json")}
    proc = subprocess.Popen([str(DRIVER)], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    check("areas path is the custom overlay's sibling", proc.stdout.readline().strip() == "areas " + str(areas))

    def groups(body):
        if body is None:
            areas.unlink(missing_ok=True)
        else:
            areas.write_text(body)
        proc.stdin.write("reload\n")
        proc.stdin.flush()
        return proc.stdout.readline().rstrip("\n").removeprefix("groups ")

    steps = [
        ("no areas.json leaves the overlay's own groups", None, "nil"),
        ("a parsed areas.json owns the groups", '{"space_groups":[{"name":"Rails","spaces":["a","B"]}]}', "Rails=a,B"),
        ("null space_groups keeps the last groups", '{"space_groups":null}', "Rails=a,B"),
        ("a non-list space_groups keeps the last groups", '{"space_groups":{"name":"X"}}', "Rails=a,B"),
        ("a group with a null name keeps the last groups", '{"space_groups":[{"name":null}]}', "Rails=a,B"),
        ("null spaces keeps the last groups", '{"space_groups":[{"name":"X","spaces":null}]}', "Rails=a,B"),
        ("a non-string member keeps the last groups", '{"space_groups":[{"name":"X","spaces":[1]}]}', "Rails=a,B"),
        ("a half-written file keeps the last groups", '{"space_groups":[{"na', "Rails=a,B"),
        ("missing fields default and blank names drop", '{"space_groups":[{"spaces":["a"]},{"name":" "},{"name":"Open"}]}', "Open="),
        ("a parsed file without space_groups clears them", '{"areas":[]}', ""),
        ("an explicit empty list clears them", '{"space_groups":[]}', ""),
        ("removing the file leaves the overlay's own groups", None, "nil"),
    ]
    for name, body, want in steps:
        got = groups(body)
        check(name, got == want, f"got {got!r}, want {want!r}")
    proc.stdin.close()
    proc.wait(timeout=10)

    env.pop("FACTORY_OVERLAY")
    out = subprocess.run([str(DRIVER)], env=env, input="", capture_output=True, text=True).stdout.splitlines()
    check("default overlay keeps the home areas.json", out[:1] == ["areas " + str(default_areas)], str(out[:1]))
    env["HERDR_AREAS_PATH"] = str(tmp / "fixture.json")
    env["FACTORY_OVERLAY"] = str(custom / "overlay.json")
    out = subprocess.run([str(DRIVER)], env=env, input="", capture_output=True, text=True).stdout.splitlines()
    check("HERDR_AREAS_PATH still names a fixture", out[:1] == ["areas " + str(tmp / "fixture.json")], str(out[:1]))

if failures:
    raise SystemExit("FAIL: " + ", ".join(failures))
print("PASS lane files")
