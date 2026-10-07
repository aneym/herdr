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
                    str(ROOT / "Sources/HerdrShell/MachineMerge.swift"),
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
check("selecting a tab opens its parked space", space("revealParked", "p")[4] == "open" and "tab:parkedtab" in selected("revealParked", "tab"))
check("selecting a PINNED row preserves its persisted home fold", "pinnedReveal|false|true" in rows and "pinned:pin1" in selected("revealPinned", "tab") and "tab:plain1" not in selected("revealPinned", "tab"))
check("selecting an AGENTS row preserves its home fold", "agentReveal|false|true" in rows and "agent:lead" in selected("revealAgent", "tab") and "tab:plain1" not in selected("revealAgent", "tab"))
check("selecting a tab unfolds its collapsed space", "tab:plain1" not in selected("folded", "tab") and "tab:plain1" in selected("revealFolded", "tab"))
check("reveal reports a change once, so a repeat select saves nothing", "reveal|true|true|false" in rows)
check("remote tab homed by label reveals the local parked space", "remote|box/w1:t1|p|true" in rows and "tab:box/w1:t1" in selected("remote-box/w1:t1", "tab"))
check("remote tab in its own parked space reveals that space", "remote|box/w2:t1|box/w2|true" in rows and "tab:box/w2:t1" in selected("remote-box/w2:t1", "tab"))
check("remote parked tabs stay folded until selected", "tab:box/w2:t1" not in selected("remote-missing", "tab") and "remote|missing|-|false" in rows)
check("goal-row and no-goal fallback inputs both list spaces", "goalPresent|true|true" in rows and "goalAbsent|false|true" in rows)
check("collapse all persists folds, preserves inner state and keeps AGENTS/PINNED", "collapseAllChrome|5|false|true|true" in rows and "agent:leadAll" in selected("collapseAll", "tab") and "pinned:pin1" in selected("collapseAll", "tab") and "tab:plain1" not in selected("collapseAll", "tab"))
check("expand all restores spaces including parked, leaving inner folds alone", "expandAllChrome|0|true|true|true" in rows and "tab:plain1" in selected("expandAll", "tab") and "tab:parkedtab" in selected("expandAll", "tab"))
for selected_tab, removed, expected in [
    ("p2", "p2", "p3"), ("p3", "p3", "p2"),
    ("a2", "a2", "a3"), ("a3", "a3", "a2"),
    ("u1", "u1", "u2"), ("p2", "p1,p2,p3", "a1"),
    ("p2", "p2,p3", "p1"),
]:
    check(f"close {selected_tab} removing {removed} focuses {expected}",
          f"close|{selected_tab}|{removed}|{expected}" in rows)
summary = f"{len(lines) - len(failures)}/{len(lines)} checks passed"
print(summary)
REPORT.write_text("\n".join(lines + [summary, ""] + rows) + "\n")
raise SystemExit(1 if failures else 0)
