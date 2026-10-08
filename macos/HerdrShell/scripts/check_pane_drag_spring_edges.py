#!/usr/bin/env python3
"""Real Mac Shell/server spring edge scenarios, driven through NSApp mouse events in Cua.

Run with HERDR_SHELL_SPACE=1 and HERDR_SHELL_BIN pointing to a pane.place-capable server.
No mocks of Shell code: frozen motion time isolates dwell boundaries, then the live display
clock is resumed. Existing check scripts are imported, never modified.
"""
import json
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import check_pane_drag_spring as P

D, S = P.D, P.S
S.OUT = str(D.ROOT / "checks/PANE-DRAG-SPRING-EDGES.txt")


def check(name, ok, state):
    D.check(name, ok, json.dumps([state.get("selected_tab"), D.pd(state)]))


def spring(ws, label):
    origin, dest, a, b, x, y, _ = P.hover_dest(ws, label)
    D.motion("freeze", ms=P.DWELL)
    D.wait(lambda s: s.get("selected_tab") == dest)
    D.wait(lambda s: set(D.pd(s).get("boxes") or {}) == {x, y})
    return origin, dest, a, b, x, y


def cleanup(*tabs):
    D.motion("run")
    D.hook("cancel", via="esc")
    D.wait(D.idle)
    for tab in tabs:
        D.close(tab)


def threshold_and_live(ws):
    origin, dest, a, b, x, y, _ = P.hover_dest(ws, "travel")
    # Locate the row through real hit-testing, not a duplicate of SwiftUI's row geometry.
    # The host coordinate system accepts sidebar points (negative x).
    point = None
    height = S.state()["host_size"][1]
    for yy in range(8, int(height), 16):
        D.hook("move", x=-100, y=yy, steps=1)
        state = S.state()
        if D.zone_is(state, kind="into_tab", tab=dest):
            point = {"x": -100, "y": yy}
            break
    if point is None:
        D.check("travel: destination row found by hit-testing", False)
        cleanup(origin, dest)
        return
    D.motion("freeze", ms=300)
    D.hook("move", x=point["x"] + 4, y=point["y"], steps=1)
    D.motion("freeze", ms=450)
    state = S.state()
    check("travel: exactly 4 pt resets dwell (no focus at old deadline)",
          P.focus_calls(state) == [] and P.lifted(state, a), state)
    D.motion("freeze", ms=749)
    state = S.state()
    check("travel: reset dwell waits its full 450 ms", P.focus_calls(state) == [], state)
    # At 449 ms elapsed, resuming must preserve elapsed time and fire on the live clock.
    D.motion("run")
    state = D.wait(lambda s: s.get("selected_tab") == dest, timeout=3)
    check("live: resumed dwell springs once and keeps the pane lifted",
          P.focus_calls(state) == [dest] and P.lifted(state, a), state)
    D.check("live: server and Shell agree", P.server_focused(ws) == [dest])
    cleanup(origin, dest)


def foreign_selection(ws):
    origin, dest, a, b, x, y = spring(ws, "foreign")
    foreign, _, _ = P.dest_tab(ws, "foreign-choice")
    before = P.focus_calls(S.state())
    # A different client owns this focus change; select mirrors it in the Shell.
    S.lab("herdr", "tab", "focus", foreign)
    S.cmd({"cmd": "select", "tab": foreign})
    D.motion("run")
    state = D.wait(D.idle)
    time.sleep(0.3)
    state = S.state()
    check("foreign: selection ends drag without restoring origin",
          D.idle(state) and state.get("selected_tab") == foreign and P.focus_calls(state) == before, state)
    D.check("foreign: server stays on foreign tab", P.server_focused(ws) == [foreign],
            json.dumps(P.server_focused(ws)))
    cleanup(origin, dest, foreign)


def spring_back(ws):
    origin, dest, a, b, x, y = spring(ws, "back")
    D.motion("freeze", ms=0)
    D.hook("move", row="tab:" + origin)
    state = S.state()
    check("source tab: no into-tab zone but drag remains lifted",
          D.pd(state).get("zone") is None and P.lifted(state, a), state)
    D.motion("freeze", ms=P.DWELL)
    state = D.wait(lambda s: s.get("selected_tab") == origin)
    check("source tab: row still springs back", P.focus_calls(state) == [dest, origin]
          and P.lifted(state, a), state)
    D.hook("cancel", via="esc")
    D.motion("run")
    state = D.wait(D.idle)
    check("back cancel: no redundant origin focus", P.focus_calls(state) == [dest, origin]
          and state.get("selected_tab") == origin and D.idle(state), state)
    D.check("back cancel: server remains at origin", P.server_focused(ws) == [origin])
    cleanup(origin, dest)


def refused(ws, unchanged=False):
    label = "unchanged spring" if unchanged else "error spring"
    origin, dest, a, b, x, y = spring(ws, label.replace(" ", "-"))
    D.hook("move", **D.edge_point(D.pd(S.state())["boxes"][y], "right"))
    D.wait(lambda s: D.zone_is(s, kind="pane_edge", target=y) and not D.pd(s).get("dryRunPending"))
    D.hook("hold-drops", on=True)
    D.hook("drop")
    D.wait(lambda s: D.pd(s).get("phase") == "dropping")
    if unchanged:
        # A zoom arriving while the drop is held makes pane.place return changed:false.
        S.lab("herdr", "pane", "zoom", x, "--on")
    else:
        # A disappeared target yields a real server error, not a fabricated reply.
        S.lab("herdr", "pane", "close", y)
    D.hook("send-drop")
    state = D.wait(lambda s: D.pd(s).get("phase") == "cancelling")
    check(f"{label}: restores origin before cancel animation",
          state.get("selected_tab") == origin and P.focus_calls(state) == [dest, origin]
          and any(m.get("kind") == "cancel" for m in D.pd(state).get("motion", {}).get("active", [])), state)
    D.motion("run")
    state = D.wait(D.idle)
    check(f"{label}: ends idle on origin", D.idle(state) and state.get("selected_tab") == origin, state)
    D.check(f"{label}: server agrees and source never moved",
            P.server_focused(ws) == [origin] and D.layout(a).get("tab_id") == origin)
    D.hook("hold-drops", on=False)
    cleanup(origin, dest)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = D.api("workspace", "list")["workspaces"]
    made = D.api("workspace", "create", "--label", "drag", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = made["workspace"]["workspace_id"]
    root = made["root_pane"]["pane_id"]
    probe = D.raw("pane", "place", root, "--beside", root, "--side", "right", "--dry-run")
    if (probe.get("result") or {}).get("place", {}).get("reason") != "same_pane":
        print("[BLOCKED] lab server does not answer pane.place")
        S.lab("down")
        raise SystemExit(2)
    S.app("start")
    S.cmd({"cmd": "activate"})
    D.wait(lambda s: s.get("window_key") is True and "paneDrag" in s)
    threshold_and_live(ws)
    foreign_selection(ws)
    spring_back(ws)
    refused(ws)
    refused(ws, unchanged=True)
    D.finish()


if __name__ == "__main__":
    main()
