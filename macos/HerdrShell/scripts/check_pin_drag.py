#!/usr/bin/env python3
"""Drag a PINNED row to reorder pins, through the app's real mouse path.

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 scripts/check_pin_drag.py.
The lab server is HERDR_SHELL_BIN (default ~/.local/bin/herdr) and needs `tab.pin_move`.
Mouse events go through NSApp.sendEvent (the drag_pin hook), so SwiftUI decides drag versus
click as it does for a physical mouse. Checks:
  - a drag up two rows moves the pin: the rows and ⌘1..9 follow at once, the server agrees,
    and the release selects nothing;
  - Esc before the release, or a release far below the section, leaves the order alone;
  - a press and release in place still selects the row.
Writes checks/PIN-DRAG.txt and before/after shots.
"""
import json
import os
import pathlib
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_pin_drag requires HERDR_SHELL_SPACE=1; host launch is forbidden")
# Short name: the Space bridge forwards sessions/<name>/herdr-client.sock, and ssh refuses
# a socket path of 104 bytes or more ("shellspike-pindrag" made it exactly 104).
os.environ["SHELL_LAB"] = "shellspike-pd"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/PIN-DRAG.txt")
lines = []
failures = []
ROW = 24  # row height plus the list's 1pt spacing


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    return state


def pinned_rows(state):
    return [row.split("|")[1][len("pinned:"):] for row in state.get("spaces_rows", []) if row.split("|")[1].startswith("pinned:")]


def server_order(ws):
    tabs = api("tab", "list", "--workspace", ws)["tabs"]
    return [tab["tab_id"] for tab in sorted((t for t in tabs if t.get("pin_index") is not None), key=lambda t: t["pin_index"])]


def drag(row, dy, esc=False):
    S.cmd({"cmd": "drag_pin", "row": "pinned:" + row, "dy": dy, "steps": 8, "interval": 0.04, "esc": esc})
    return wait(lambda s: s.get("drag_running") is False and s.get("pin_drag", {}).get("dragged") is None, timeout=10)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "pins", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = made["workspace"]["workspace_id"]
    tabs = [made["tab"]["tab_id"]] + [api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
                                      for name in ("beta", "gamma", "delta")]
    for tab in tabs[:3]:
        S.lab("herdr", "tab", "pin", tab)
    a, b, c = tabs[:3]
    S.app("start")
    # SwiftUI drops synthesized mouse events on a window that is not key; the Space is
    # the app's own desktop, so bringing it front takes nothing from anyone.
    S.cmd({"cmd": "activate"})
    wait(lambda s: s.get("window_key") is True, timeout=10)
    state = wait(lambda s: pinned_rows(s) == [a, b, c])
    check("pins listed in server order", pinned_rows(state) == [a, b, c], str(pinned_rows(state)))
    S.cmd({"cmd": "select", "tab": tabs[3]})
    state = wait(lambda s: s.get("selected_tab") == tabs[3])
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/PIN-DRAG-before.png")})

    state = drag(c, -2 * ROW)
    check("drag up two rows reorders at once", pinned_rows(state) == [c, a, b], str(pinned_rows(state)))
    check("cmd 1..3 follow the new order", state.get("numbered_tabs", [])[:3] == [c, a, b], str(state.get("numbered_tabs")))
    check("the drag selects nothing", state.get("selected_tab") == tabs[3], str(state.get("selected_tab")))
    deadline = time.monotonic() + 10
    while server_order(ws) != [c, a, b] and time.monotonic() < deadline:
        time.sleep(0.2)
    check("server holds the dragged order", server_order(ws) == [c, a, b], str(server_order(ws)))
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/PIN-DRAG-after.png")})

    state = drag(c, 2 * ROW, esc=True)
    time.sleep(0.5)
    state = S.state()
    check("esc cancels the drag", pinned_rows(state) == [c, a, b] and server_order(ws) == [c, a, b], str(pinned_rows(state)))

    state = drag(a, 12 * ROW)
    time.sleep(0.5)
    state = S.state()
    check("a release off the section moves nothing", pinned_rows(state) == [c, a, b] and server_order(ws) == [c, a, b], str(pinned_rows(state)))

    state = drag(b, 0)
    state = wait(lambda s: s.get("selected_tab") == b, timeout=5)
    check("a press in place still selects the row", state.get("selected_tab") == b, str(state.get("selected_tab")))

    S.app("stop")
    S.lab("down")
    pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
