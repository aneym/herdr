#!/usr/bin/env python3
"""Lead-run PC acceptance for pane drag; respects pc.py's game guard.

Uses throwaway workspaces only, with real cap pointer events and server layouts.
This script is intentionally not run by the S7 implementation seat.
"""
import argparse
import json
import pathlib
import subprocess
import sys
import time

import pc

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE.parent / "checks" / "PANE-DRAG.txt"
EVIDENCE = pathlib.Path.home() / ".agent-rails/lanes/herdr-ui/evidence/pane-drag"
lines = []
failures = []


def herdr(*args):
    out = subprocess.check_output(["herdr", *args], text=True)
    return json.loads(out)["result"]


TEST_WINDOW = False


def ctl(payload):
    rc, out = pc.ctl_send(payload, timeout=60, test_window=TEST_WINDOW)
    reply = json.loads(out.splitlines()[-1], strict=False) if out else {}
    if rc or reply.get("ok") is False:
        raise RuntimeError(f"{payload.get('cmd')}: {out}")
    return reply


def check(name, condition):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}"
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def layout(pane):
    return herdr("pane", "layout", "--pane", pane)["layout"]


def wait(predicate):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(.2)
    return False


def shot(name):
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    switches = ["--test-window"] if TEST_WINDOW else []
    subprocess.run(["python3", str(HERE / "pc.py"), "shot", "--out", str(EVIDENCE / f"s7-{name}.png"), *switches], check=True)


def drag(source, to, **extra):
    return ctl({"cmd": "drag_pane", "pane_id": source, "to": to, "steps": 8, "interval_ms": 40, **extra})


def main(argv=()):
    global TEST_WINDOW
    ap = argparse.ArgumentParser()
    ap.add_argument("--test-window", action="store_true", help="target the isolated test window pipe")
    TEST_WINDOW = ap.parse_args(argv).test_window
    pc.bootstrap()
    if pc.guard(quiet=True)[0]:
        print("game guard holds; PC acceptance deferred")
        raise SystemExit(75)
    start = ctl({"cmd": "ui"})
    made = herdr("workspace", "create", "--label", "pane-drag-check", "--no-focus")
    other = herdr("workspace", "create", "--label", "pane-drag-destination", "--no-focus")
    workspaces = [made["workspace"]["workspace_id"], other["workspace"]["workspace_id"]]
    a = made["root_pane"]["pane_id"]
    tab = made["tab"]["tab_id"]
    gated = False
    try:
        b = herdr("pane", "split", "--pane", a, "--direction", "right", "--no-focus")["pane"]["pane_id"]
        c = herdr("pane", "split", "--pane", b, "--direction", "down", "--no-focus")["pane"]["pane_id"]
        second = herdr("tab", "create", "--workspace", workspaces[0], "--label", "pane-drag-tab", "--no-focus")["tab"]["tab_id"]
        ctl({"cmd": "open", "tab_id": tab})
        check("A | (B / C) visible", wait(lambda: len(ctl({"cmd": "ui"}).get("panes", [])) == 3))
        for name, to in [("centre", {"pane_id": b, "zone": "centre"}), ("pane-edge", {"pane_id": b, "zone": "right"}), ("tab-edge", {"tab_edge": "down"}), ("tab-row", {"tab_id": second})]:
            drag(a, to, hold=True)
            shot(name)
            ctl({"cmd": "key", "key": "Escape"})
            ctl({"cmd": "drag_pane", "release": True})
        for name, flags, to in [("Esc", {"esc": True}, {"pane_id": b, "zone": "centre"}), ("right-click", {"right_click": True}, {"pane_id": b, "zone": "centre"}), ("source off-target", {}, {"pane_id": a, "zone": "centre"})]:
            before = layout(a)
            drag(a, to, **flags)
            time.sleep(.3)
            check(f"{name} cancels without changing layout", layout(a) == before)
        before = layout(a)
        drag(a, {"pane_id": b, "zone": "right"})
        check("pane-edge rearranges server layout", wait(lambda: layout(a) != before))
        before = layout(a)
        positions = {p["pane_id"]: p["rect"] for p in before["panes"]}
        drag(a, {"pane_id": b, "zone": "centre"})
        check("centre swaps source and target", wait(lambda: {p["pane_id"]: p["rect"] for p in layout(a)["panes"]}.get(a) == positions[b]))
        before = layout(a)
        drag(a, {"tab_edge": "down"})
        for ms in (0, 100, 200):
            ctl({"cmd": "motion", "freeze_ms": ms})
            shot(f"settle-{ms}")
        ctl({"cmd": "motion", "freeze_ms": None})
        check("tab-edge rearranges server layout", wait(lambda: layout(a) != before))
        drag(a, {"tab_id": second})
        check("tab row moves to destination tab", wait(lambda: herdr("pane", "get", a)["pane"]["tab_id"] == second))
        ctl({"cmd": "open", "tab_id": tab})
        drag(b, {"space_id": workspaces[1]})
        check("space header creates a tab in destination", wait(lambda: herdr("pane", "get", b)["pane"]["workspace_id"] == workspaces[1]))
        check("remaining pane still exists", herdr("pane", "get", c)["pane"]["pane_id"] == c)
    except pc.Gated:
        gated = True
        raise SystemExit(75)
    except Exception as error:
        check(f"scenario completes: {error}", False)
    finally:
        for workspace in workspaces:
            herdr("workspace", "close", workspace)
        if start.get("selected_tab") and not gated:
            ctl({"cmd": "open", "tab_id": start["selected_tab"]})
        OUT.parent.mkdir(parents=True, exist_ok=True)
        OUT.write_text("\n".join(lines) + "\n")
        EVIDENCE.mkdir(parents=True, exist_ok=True)
        (EVIDENCE / "s7-pc.txt").write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main(sys.argv[1:])
