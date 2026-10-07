#!/usr/bin/env python3
"""Drag pinned rows and a split divider on the PC Shell, through the app's own pointer path.

Runs on Studio against the installed PC app (`pc.py ctl`) and the Studio herdr server it is
attached to. The drag_pin hook presses, moves and releases at real points in the WebView, so
the sidebar's handlers tell a drag from a click as for a physical mouse. It works on three
throwaway tabs in a throwaway workspace and closes that workspace at the end. They pin as
agent chats (`tab set-role agent`), not plain pins: pinning a plain tab under a configured
[ui.sidebar.priority] re-sorts every plain pin, which could reorder Alex's own. AGENTS and
PINNED rows share one drag path; the slot rules of both are in app/src/pinDrag.test.ts.
Alex's agent and plain pins are checked to keep their order. Checks, as the Mac's
macos/HerdrShell/scripts/check_pin_drag.py:
  - a drag up two rows moves the pin: the rows and Ctrl+1..9 follow at once, the server
    agrees, and the release selects nothing;
  - Esc before the release, or a release far below the section, leaves the order alone;
  - a press and release in place still selects the row;
  - dragging a split divider right grows the left pane's ratio on the server.
Refuses to run while a game is running. Writes windows/HerdrShell/checks/PIN-DRAG.txt.
"""
import json
import pathlib
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pc  # noqa: E402

OUT = HERE.parent / "checks" / "PIN-DRAG.txt"
lines = []
failures = []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def herdr(*args):
    out = subprocess.run(["herdr", *args], capture_output=True, text=True, timeout=30, check=True).stdout
    return json.loads(out)["result"] if out.strip().startswith("{") else out


def ctl(obj):
    rc, out = pc.ctl_send(obj, timeout=60)
    try:
        reply = json.loads(out.splitlines()[-1]) if out else {}
    except json.JSONDecodeError:
        reply = {}
    if rc != 0 or reply.get("ok") is False:
        raise SystemExit(f"ctl {obj.get('cmd')} failed: rc={rc} {out[:300]}")
    return reply


def server_pins(agent=True):
    tabs = herdr("tab", "list")["tabs"]
    return [t["tab_id"] for t in sorted((t for t in tabs if t.get("pin_index") is not None and (t.get("role") == "agent") == agent), key=lambda t: t["pin_index"])]


def ui_pins(state):
    return [r["id"] for r in state.get("rows", []) if r["kind"] == "agent"]


def wait(predicate, timeout=10):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = ctl({"cmd": "ui"})
        if predicate(state):
            return state
        time.sleep(0.3)
    return state


def drag(tab, rows, esc=False):
    ctl({"cmd": "drag_pin", "tab_id": tab, "section": "agent", "rows": rows, "steps": 8, "interval_ms": 40, "esc": esc})
    return ctl({"cmd": "ui"})


def main():
    pc.bootstrap()
    if pc.guard(quiet=True)[0]:
        raise SystemExit("a game is running on the PC; not driving the app")
    commit = ctl({"cmd": "ping"}).get("commit")
    start = ctl({"cmd": "ui"})
    selected_before = start.get("selected_tab")
    alex_before = (server_pins(True), server_pins(False))
    gated = False
    made = herdr("workspace", "create", "--label", "drag-check", "--no-focus")
    ws = made["workspace"]["workspace_id"]
    try:
        created = [herdr("tab", "create", "--workspace", ws, "--label", name, "--no-focus") for name in ("drag-b", "drag-c", "drag-free")]
        tabs = [made["tab"]["tab_id"]] + [c["tab"]["tab_id"] for c in created]
        for tab in tabs[:3]:
            herdr("tab", "set-role", tab, "agent")
        pins = [t for t in server_pins() if t in tabs[:3]]
        a, b, c = pins
        state = wait(lambda s: [t for t in ui_pins(s) if t in pins] == pins)
        mine = lambda s: [t for t in ui_pins(s) if t in pins]  # noqa: E731
        check("throwaway pins listed in server order", mine(state) == pins, str(mine(state)))
        check("throwaway pins sit together", "".join("x" if t in pins else "." for t in ui_pins(state)).count("xxx") == 1, str(ui_pins(state)))
        ctl({"cmd": "open", "tab_id": tabs[3]})
        wait(lambda s: s.get("selected_tab") == tabs[3])

        state = drag(c, -2)
        check("drag up two rows reorders at once", mine(state) == [c, a, b], str(mine(state)))
        # Ctrl+1..9 run out after nine rows; compare the numbers the throwaway rows did get.
        hot = {r["id"]: r["hotkey"] for r in state["rows"] if r["kind"] == "agent" and r["id"] in pins}
        numbered = [hot[t] for t in (c, a, b) if hot.get(t) is not None]
        check("ctrl numbers follow the new order", numbered == sorted(numbered) and (not numbered or hot.get(c) is not None), str(hot))
        check("the drag selects nothing", state.get("selected_tab") == tabs[3], str(state.get("selected_tab")))
        deadline = time.monotonic() + 10
        while [t for t in server_pins() if t in pins] != [c, a, b] and time.monotonic() < deadline:
            time.sleep(0.3)
        order = [t for t in server_pins() if t in pins]
        check("server holds the dragged order", order == [c, a, b], str(order))
        time.sleep(4.5)  # past the 4 s a dropped order is shown without the server
        state = ctl({"cmd": "ui"})
        check("the order still holds once only the server shows it", mine(state) == [c, a, b], str(mine(state)))

        state = drag(c, 2, esc=True)
        time.sleep(0.5)
        state = ctl({"cmd": "ui"})
        order = [t for t in server_pins() if t in pins]
        check("esc cancels the drag", mine(state) == [c, a, b] and order == [c, a, b], f"{mine(state)} {order}")

        state = drag(a, 12)
        time.sleep(0.5)
        state = ctl({"cmd": "ui"})
        order = [t for t in server_pins() if t in pins]
        check("a release off the section moves nothing", mine(state) == [c, a, b] and order == [c, a, b], f"{mine(state)} {order}")

        drag(b, 0)
        state = wait(lambda s: s.get("selected_tab") == b, timeout=5)
        check("a press in place still selects the row", state.get("selected_tab") == b, str(state.get("selected_tab")))

        # A split divider drags too, as on the Mac: pane.resize moves herdr's own ratio.
        root = created[2]["root_pane"]["pane_id"]
        herdr("pane", "split", "--pane", root, "--direction", "right", "--no-focus")
        ctl({"cmd": "open", "tab_id": tabs[3]})
        split = lambda: herdr("pane", "layout", "--pane", root)["layout"]["splits"][0]  # noqa: E731
        before = split()
        wait(lambda s: s.get("selected_tab") == tabs[3] and len(s.get("panes", [])) == 2)
        time.sleep(0.5)
        ctl({"cmd": "drag_divider", "split_id": before["id"], "delta": 120, "steps": 8, "interval_ms": 40})
        deadline = time.monotonic() + 5
        while split()["ratio"] <= before["ratio"] + 0.05 and time.monotonic() < deadline:
            time.sleep(0.3)
        after = split()
        check("a divider drag right grows the left pane on the server", after["ratio"] > before["ratio"] + 0.05, f"{before['ratio']} -> {after['ratio']}")
    except pc.Gated:
        gated = True
        raise
    except BaseException as error:  # noqa: BLE001 - recorded, then the cleanup and pin check still run
        check("the run completes", False, repr(error))
    finally:
        herdr("workspace", "close", ws)
        # A game started: the server-side cleanup above runs, nothing more reaches the app.
        if selected_before and not gated:
            try:
                ctl({"cmd": "open", "tab_id": selected_before})
            except SystemExit:
                pass
        alex_after = (server_pins(True), server_pins(False))
        check("Alex's pins keep their order", alex_after == alex_before, f"{alex_before} -> {alex_after}")
    lines.insert(0, f"app commit {commit}")
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
