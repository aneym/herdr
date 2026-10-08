#!/usr/bin/env python3
"""Spring-loaded tabs while a pane is lifted (pane drag slice S8; owner-written, do not edit).

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 macos/HerdrShell/scripts/check_pane_drag_spring.py
(or check_pane_drag.py --spring, which runs this file). Same lab server rules as check_pane_drag.py:
HERDR_SHELL_BIN must answer pane.place, else BLOCKED (exit 2).

Spec pane-drag-rearrange-2026-10-07, "Spring-loaded tabs" and S8: a lifted pane resting on a same-machine
sidebar tab row for ShellMotion.springLoadMs (450 ms) with under 4 pt of travel sends `tab.focus` for that tab;
the Shell shows it and the drag continues into its zones, so a drop places into one of its panes (never
IntoTab). Leaving the row before 450 ms resets the dwell. A cancel after a spring-load sends `tab.focus` back
to the origin tab.

Hooks this check drives, all existing except where S8 must add behaviour:
  {"cmd":"pane-drag","op":"begin","pane":P} / {"op":"move","row":"tab:T"} / {"op":"move","x":X,"y":Y}
  {"cmd":"pane-drag","op":"drop"} / {"op":"cancel","via":"esc"|"right"}
  {"cmd":"motion","op":"freeze","ms":N} / {"cmd":"motion","op":"run"}
S8 adds, with no new hook verb:
  1. The dwell is a PaneDrag motion clock: under `motion freeze ms:N` its elapsed time reads N, as every
     pane drag motion's does, and `PaneDrag.freeze(ms:)` evaluates the dwell at once (fires it when N >= 450
     and the pointer has rested on the row since freeze 0). Under `motion run` the dwell runs on real time.
     A hover that starts under freeze stamps the dwell at the frozen clock's current value.
  2. The spring sends `tab.focus {tab_id: T}` through the controller's calls, so it appears in the state
     dump's `paneDrag.sent` as {"method":"tab.focus","params":{"tab_id":T}}, and the server focuses T.
     The Shell selects T (`selected_tab` == T); `selectionChanging(to:)` must not cancel a drag that a
     spring-load caused. The drag stays lifted (`paneDrag.phase` "lifted", `source` A) and
     `paneDrag.boxes` become T's panes.
  3. A cancel (Esc or right click) after a spring-load appends `tab.focus {tab_id: origin}` to `sent`, the
     Shell selects the origin again and the server focuses it.
Writes macos/HerdrShell/checks/PANE-DRAG-SPRING.txt.
"""
import json
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import check_pane_drag as D  # noqa: E402  (sets the lab name, the server binary and the Space guard)

S = D.S
S.OUT = str(D.ROOT / "checks/PANE-DRAG-SPRING.txt")
DWELL = 450  # ShellMotion.springLoadMs


def focus_calls(state):
    return [c.get("params", {}).get("tab_id") for c in D.sent(state) if c.get("method") == "tab.focus"]


def server_focused(ws):
    return [t["tab_id"] for t in D.api("tab", "list", "--workspace", ws)["tabs"] if t.get("focused")]


def lifted(state, a):
    return D.pd(state).get("phase") == "lifted" and D.pd(state).get("source") == a


def dest_tab(ws, label):
    """A tab X | Y, not focused on the server; returns (tab, x, y)."""
    made = D.api("tab", "create", "--workspace", ws, "--label", label, "--no-focus")
    tab, x = made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    y = D.split(x, "right")
    D.wait(lambda s: any(r.split("|")[1] == "tab:" + tab for r in s.get("spaces_rows", [])))
    return tab, x, y


def hover_dest(ws, label):
    """Fresh origin A | (B / C) selected, a dest tab X | Y; A lifted and resting on dest's row at frozen 0."""
    origin, a, b, c, state = D.fresh(ws, label)
    dest, x, y = dest_tab(ws, label + "-dest")
    D.S.cmd({"cmd": "select", "tab": origin})
    state = D.wait(lambda s: s.get("selected_tab") == origin and D.idle(s))
    focus0 = server_focused(ws)
    D.motion("freeze", ms=0)
    D.hook("begin", pane=a)
    D.hook("move", row="tab:" + dest)
    state = D.wait(lambda s: D.pd(s).get("zone") is not None, timeout=5)
    D.check(f"{label}: over dest's row the zone is into that tab",
            D.zone_is(state, kind="into_tab") and lifted(state, a), json.dumps([D.pd(state).get("zone"), D.pd(state).get("phase")]))
    return origin, dest, a, b, x, y, focus0


def dwell_to_focus(ws):
    origin, dest, a, b, x, y, focus0 = hover_dest(ws, "spring")
    D.motion("freeze", ms=DWELL - 150)
    time.sleep(0.3)
    state = S.state()
    D.check("dwell: at 300 ms no tab.focus and the origin stays selected",
            focus_calls(state) == [] and state.get("selected_tab") == origin and lifted(state, a),
            json.dumps([focus_calls(state), state.get("selected_tab"), D.pd(state).get("phase")]))
    D.motion("freeze", ms=DWELL)
    state = D.wait(lambda s: focus_calls(s) == [dest] and s.get("selected_tab") == dest, timeout=5)
    D.check("dwell: at 450 ms exactly one tab.focus(dest) is sent and the Shell shows dest",
            focus_calls(state) == [dest] and state.get("selected_tab") == dest,
            json.dumps([focus_calls(state), state.get("selected_tab")]))
    D.check("dwell: the server focuses dest", server_focused(ws) == [dest], json.dumps([server_focused(ws), focus0]))
    D.check("dwell: the pane stays lifted", lifted(state, a), json.dumps([D.pd(state).get("phase"), D.pd(state).get("source")]))
    D.motion("run")
    state = D.wait(lambda s: set(D.pd(s).get("boxes") or {}) == {x, y} and lifted(s, a), timeout=5)
    D.check("dwell: the drag continues over dest's panes", set(D.pd(state).get("boxes") or {}) == {x, y} and lifted(state, a),
            json.dumps([sorted(D.pd(state).get("boxes") or {}), D.pd(state).get("phase")]))
    time.sleep(0.6)
    D.check("dwell: it springs once", focus_calls(S.state()) == [dest], json.dumps(focus_calls(S.state())))
    D.hook("move", **D.edge_point(D.pd(state)["boxes"][y], "right"))
    state = D.wait(lambda s: D.zone_is(s, kind="pane_edge", target=y, side="right") and not D.pd(s).get("dryRunPending"), timeout=5)
    dry = [c["params"] for c in D.dry_runs(state)]
    D.check("dwell: dest's pane edge asks a dry run against that pane",
            D.zone_is(state, kind="pane_edge", target=y, side="right")
            and any(p.get("target") == {"type": "pane", "pane_id": y} and p.get("side") == "right" for p in dry),
            json.dumps([D.pd(state).get("zone"), dry]))
    D.hook("drop")
    state = D.wait(lambda s: D.idle(s) and D.changes(s), timeout=5)
    places = [c["params"] for c in D.changes(state) if c.get("method") == "pane.place"]
    D.check("dwell: the drop places A beside dest's pane, not into the tab",
            len(places) == 1 and places[0].get("target") == {"type": "pane", "pane_id": y}
            and places[0].get("side") == "right" and places[0].get("dry_run") is False,
            json.dumps(D.changes(state)))
    lay = D.layout(a)
    D.check("dwell: the server put A in dest right of Y", lay.get("tab_id") == dest
            and D.rects(lay)[a][0] > D.rects(lay)[y][0], json.dumps([lay.get("tab_id"), D.rects(lay)]))
    D.close(origin)
    D.close(dest)


def leave_early(ws):
    origin, dest, a, b, x, y, focus0 = hover_dest(ws, "leave")
    D.motion("freeze", ms=300)
    time.sleep(0.2)
    boxes = D.pd(S.state()).get("boxes") or {}
    D.hook("move", **D.centre(boxes[b]))
    state = D.wait(lambda s: D.zone_is(s, kind="centre", target=b), timeout=5)
    D.check("leave: the pointer left the row for B's centre", D.zone_is(state, kind="centre", target=b),
            json.dumps(D.pd(state).get("zone")))
    D.motion("freeze", ms=DWELL)
    D.motion("freeze", ms=2000)
    D.motion("run")
    time.sleep(0.8)
    state = S.state()
    D.check("leave: leaving at 300 ms sends no tab.focus and the origin stays selected",
            focus_calls(state) == [] and state.get("selected_tab") == origin and lifted(state, a),
            json.dumps([focus_calls(state), state.get("selected_tab"), D.pd(state).get("phase")]))
    D.hook("cancel", via="esc")
    D.wait(lambda s: D.pd(s).get("phase") != "lifted", timeout=5)
    D.hook("drop")
    state = D.wait(D.idle, timeout=5)
    D.check("leave: the cancel sends nothing", D.changes(state) == [] and focus_calls(state) == [],
            json.dumps(D.changes(state)))
    D.check("leave: the server's focused tab is unchanged", server_focused(ws) == focus0,
            json.dumps([server_focused(ws), focus0]))
    D.close(origin)
    D.close(dest)


def cancel_restores(ws, how):
    origin, dest, a, b, x, y, _ = hover_dest(ws, "cancel-" + how)
    before = D.rects(D.layout(a))
    D.motion("freeze", ms=DWELL)
    state = D.wait(lambda s: s.get("selected_tab") == dest, timeout=5)
    D.motion("run")
    D.wait(lambda s: set(D.pd(s).get("boxes") or {}) == {x, y}, timeout=5)
    D.check(f"cancel {how}: precondition: the spring showed dest", focus_calls(state) == [dest],
            json.dumps([focus_calls(state), state.get("selected_tab")]))
    D.hook("cancel", via=how)
    D.wait(lambda s: D.pd(s).get("phase") != "lifted", timeout=5)
    D.hook("drop")
    state = D.wait(lambda s: D.idle(s) and s.get("selected_tab") == origin, timeout=5)
    time.sleep(0.3)
    state = S.state()
    D.check(f"cancel {how}: tab.focus goes back to the origin and nothing else changes",
            focus_calls(state) == [dest, origin] and [c for c in D.changes(state) if c.get("method") != "tab.focus"] == [],
            json.dumps(D.changes(state)))
    D.check(f"cancel {how}: the Shell shows the origin again", state.get("selected_tab") == origin and D.idle(state),
            json.dumps([state.get("selected_tab"), D.pd(state).get("phase")]))
    D.check(f"cancel {how}: the server focuses the origin", server_focused(ws) == [origin], json.dumps(server_focused(ws)))
    D.check(f"cancel {how}: the origin's layout is unchanged", D.rects(D.layout(a)) == before, json.dumps(D.rects(D.layout(a))))
    D.close(origin)
    D.close(dest)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = D.api("workspace", "list")["workspaces"]
    drag_ws = D.api("workspace", "create", "--label", "drag", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = drag_ws["workspace"]["workspace_id"]
    root = drag_ws["root_pane"]["pane_id"]
    probe = D.raw("pane", "place", root, "--beside", root, "--side", "right", "--dry-run")
    if (probe.get("result") or {}).get("place", {}).get("reason") != "same_pane":
        print(f"[BLOCKED] lab server does not answer pane.place: {json.dumps(probe)[:300]}")
        S.lab("down")
        raise SystemExit(2)
    S.app("start")
    S.cmd({"cmd": "activate"})
    state = D.wait(lambda s: s.get("window_key") is True and "paneDrag" in s)
    if "paneDrag" not in state:
        D.check("the state dump has paneDrag", False)
        return D.finish()
    dwell_to_focus(ws)
    leave_early(ws)
    cancel_restores(ws, "esc")
    cancel_restores(ws, "right")
    D.finish()


if __name__ == "__main__":
    main()
