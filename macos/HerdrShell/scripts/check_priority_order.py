#!/usr/bin/env python3
"""Pure priority-order edge cases and snapshot/chrome JSON integration, with no app launch.

Compiles the production tree and snapshot decoder with a dump driver. This owns the
rank, tie, pin-partition, overlay-precedence and persisted parked-fold contracts;
the existing machine check does not exercise ranks or parked state. No test seams.
Writes checks/PRIORITY-ORDER.txt, including a launch-timeout blocker if applicable.
"""
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPORT = ROOT / "checks/PRIORITY-ORDER.txt"
lines, failures = [], []


def check(name, ok):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}"
    print(line)
    lines.append(line)
    if not ok:
        failures.append(name)


with tempfile.TemporaryDirectory(prefix="herdr-priority-") as scratch:
    driver = pathlib.Path(scratch) / "priority_order_dump"
    subprocess.run(["swiftc", str(ROOT / "Sources/HerdrShell/SpacesTree.swift"),
                    str(ROOT / "Sources/HerdrShell/Snapshot.swift"),
                    str(ROOT / "Sources/HerdrShell/DeskModel.swift"),
                    str(ROOT / "scripts/priority_order_dump.swift"), "-o", str(driver)],
                   check=True, timeout=120)
    try:
        rows = subprocess.check_output([str(driver)], text=True, timeout=60).splitlines()
    except subprocess.TimeoutExpired:
        REPORT.write_text("[BLOCKED] Compiled priority driver hung at launch over 60 seconds; not retried.\n")
        raise SystemExit("BLOCKED: compiled priority driver hung at launch over 60 seconds; not retried")


def selected(scenario, kind):
    return [r.split("|")[2] for r in rows if r.startswith(f"{scenario}|{kind}|")]


def space(scenario, identifier):
    return next(r.split("|") for r in rows if r.startswith(f"{scenario}|space|space:{identifier}|"))


check("spaces ranked stably, ties retain input order", selected("rank", "space") == ["space:d", "space:b", "space:c", "space:a", "space:p"])
check("client pinned-space partition precedes rank", selected("partition", "space") == ["space:b", "space:d", "space:c", "space:a", "space:p"])
check("explicit overlay member order wins over rank", selected("overlay", "space") == ["space:a", "space:c", "space:d", "space:b", "space:p"])
check("tabs ranked only; pins do not reorder equal-rank ties", selected("rank", "tab") == ["pinned:latepin", "pinned:pin2", "pinned:pin1", "tab:plain1", "tab:pin1", "tab:pin2", "tab:plain2", "tab:latepin"])
check("server pin order unchanged", [x for x in selected("rank", "tab") if x.startswith("pinned:")] == ["pinned:latepin", "pinned:pin2", "pinned:pin1"])
check("real snapshot decodes optional sort_rank and parked", "snapshot|0|false|0|4294967295|true|7" in rows)
check("old SpacesInput defaults new fields", "input|0|false|0" in rows)
check("old SpacesChrome JSON decodes and preserves prior state", "chrome|true|true" in rows)
check("parked space collapsed in place by default", space("rank", "p")[4] == "closed" and "tab:parkedtab" not in selected("rank", "tab"))
check("persisted parked expansion exposes tabs", space("expanded", "p")[4] == "open" and "tab:parkedtab" in selected("expanded", "tab") and "expandedChrome|true|true" in rows)
check("parked toggle refolds without changing collapsedSpaces", space("refolded", "p")[4] == "closed" and "tab:parkedtab" not in selected("refolded", "tab") and "refoldedChrome|true|true" in rows)
summary = f"{len(lines) - len(failures)}/{len(lines)} checks passed"
print(summary)
REPORT.write_text("\n".join(lines + [summary, ""] + rows) + "\n")
raise SystemExit(1 if failures else 0)
