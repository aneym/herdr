#!/usr/bin/env python3
"""Every pane drop ends (pane drag fix S6b): regressions beside check_pane_drag.py, on its helpers.

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 macos/HerdrShell/scripts/check_pane_drag_end.py
Same lab server rules as check_pane_drag.py (HERDR_SHELL_BIN must answer pane.place; else BLOCKED, exit 2).

  1. Reduce Motion, a release on a sidebar tab row: the source leaves the tab, so the layout brings
     no crossfade; the drop still ends (phase idle, no motion) instead of sticking in settling.
  2. The same for a release on a space header (a new tab in that space).
  3. A drop the server refuses (a centre swap with a pane closed under the drag) plays the cancel:
     the phase passes through cancelling with a cancel motion, then ends idle with the layout as it was.
S10 (a drop ends with the server's reply, never with a guess from snapshots). The hook holds the drop's call
unsent, as a slow link would, while other clients change the tab:
  4. An edge drop pending while another client resizes the source: that snapshot applies at once with no settle
     and the drop stays pending; the reply then settles the panes onto the server's layout.
  5. A swap refused after another client's snapshot moved the source: the refusal still plays the cancel.
  6. The target resized while the drop is pending and again right after it lands: the drop ends on its reply and
     the later snapshot that disagrees with the reply wins, so the boxes end on the server's layout.
S10a. The hook also holds a reply that is in until the check releases it, so the drop's own snapshot lands first:
  7. A swap whose snapshot lands before its reply: the snapshot applies at once with no settle; the reply then
     settles a and b from where they stood at the release; a later snapshot during that settle applies with no motion.
  8. Another tab selected while a drop is pending: the drop ends quietly (no cancel, no chip) and a late reply
     changes nothing on the new tab.
Writes macos/HerdrShell/checks/PANE-DRAG-END.txt.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import check_pane_drag as D  # noqa: E402  (sets the lab name, the server binary and the Space guard)

S = D.S
S.OUT = str(D.ROOT / "checks/PANE-DRAG-END.txt")


def active(state):
    return [m.get("kind") for m in (D.pd(state).get("motion") or {}).get("active", [])]


def reduced_row_drop(ws, row, label):
    tab, a, b, c, state = D.fresh(ws, label)
    D.wait(lambda s: any(r.split("|")[1] == row for r in s.get("spaces_rows", [])))
    D.motion("reduce", on=True)
    D.hook("begin", pane=a)
    D.hook("move", row=row, drop=True)
    state = D.wait(lambda s: D.idle(s) and D.changes(s), timeout=5)
    D.check(f"{label}: under Reduce Motion the drop ends (phase idle, no motion)", D.idle(state),
            json.dumps([D.pd(state).get("phase"), active(state)]))
    D.check(f"{label}: the source left the tab", set(D.rects(D.layout(b))) == {b, c}, json.dumps(D.rects(D.layout(b))))
    D.motion("reduce", on=None)
    D.close(tab)


def boxes_on_server(state, pane):
    """The Shell's pane boxes are the server layout's rects for the tab of `pane`, in host points."""
    want = D.host_rects(D.layout(pane), state)
    got = D.pd(state).get("boxes") or {}
    return set(got) == set(want) and all(D.near(got[p], r) for p, r in want.items())


def resize(pane, direction, amount=0.1):
    S.lab("herdr", "pane", "resize", "--pane", pane, "--direction", direction, "--amount", str(amount))


def held_drop(ws, label, zone_of):
    """A fresh a | (b / c) tab, a lifted onto zone_of(boxes, b) with its dry run answered, released with the call held."""
    tab, a, b, c, state = D.fresh(ws, label)
    D.hook("begin", pane=a)
    D.hook("move", steps=1, **zone_of(D.pd(state)["boxes"][b]))
    D.wait(lambda s: D.pd(s).get("zone") is not None and not D.pd(s).get("dryRunPending"))
    D.hook("hold-drops", on=True)
    D.hook("drop")
    state = D.wait(lambda s: D.pd(s).get("phase") == "dropping", timeout=5)
    D.hook("hold-drops", on=False)
    return tab, a, b, c, state


def pending_foreign_resize(ws):
    tab, a, b, c, state = held_drop(ws, "pending-resize", lambda r: D.edge_point(r, "right"))
    old = D.rects(D.layout(a))
    resize(a, "left")
    D.wait(lambda s: D.rects(D.layout(a)) != old, timeout=5)
    state = D.wait(lambda s: boxes_on_server(s, a), timeout=5)
    D.check("pending: another client's resize of the source shows at once", boxes_on_server(state, a),
            json.dumps([D.pd(state).get("boxes"), D.host_rects(D.layout(a), state)]))
    D.check("pending: that snapshot brings no settle and the drop stays pending",
            D.pd(state).get("phase") == "dropping" and not D.settling(state),
            json.dumps([D.pd(state).get("phase"), active(state)]))
    D.motion("freeze", ms=0)
    D.hook("send-drop")
    state = D.wait(lambda s: D.settling(s), timeout=5)
    D.check("pending: the reply settles the panes it moved", {a, b} <= D.settling(state), json.dumps(active(state)))
    D.motion("run")
    state = D.wait(D.idle, timeout=5)
    final = D.rects(D.layout(a))
    D.check("pending: the drop ends idle with a placed right of b at b's height",
            D.idle(state) and final[a][0] > final[b][0] and final[a][1] == final[b][1] and final[a][3] == final[b][3],
            json.dumps([D.pd(state).get("phase"), final]))
    D.check("pending: the boxes end on the server's layout", boxes_on_server(state, a),
            json.dumps([D.pd(state).get("boxes"), D.host_rects(D.layout(a), state)]))
    D.close(tab)


def refused_after_foreign(ws):
    tab, a, b, c, state = held_drop(ws, "refused-foreign", D.centre)
    old = D.rects(D.layout(a))
    resize(a, "left")
    D.wait(lambda s: D.rects(D.layout(a)) != old, timeout=5)
    state = D.wait(lambda s: boxes_on_server(s, a), timeout=5)
    D.check("refused-foreign: the resize of the source shows at once, with no settle, the drop pending",
            boxes_on_server(state, a) and D.pd(state).get("phase") == "dropping" and not D.settling(state),
            json.dumps([D.pd(state).get("phase"), active(state)]))
    S.lab("herdr", "pane", "close", b)
    state = D.wait(lambda s: b not in (D.pd(s).get("boxes") or {}) and boxes_on_server(s, a), timeout=5)
    closed = D.rects(D.layout(a))
    D.motion("freeze", ms=0)
    D.hook("send-drop")
    state = D.wait(lambda s: D.pd(s).get("phase") != "dropping", timeout=5)
    D.check("refused-foreign: the refusal plays the cancel",
            D.pd(state).get("phase") == "cancelling" and "cancel" in active(state),
            json.dumps([D.pd(state).get("phase"), active(state)]))
    D.motion("run")
    state = D.wait(D.idle, timeout=5)
    D.check("refused-foreign: nothing moved; the boxes stay on the server's layout",
            D.idle(state) and D.rects(D.layout(a)) == closed and boxes_on_server(state, a),
            json.dumps([closed, D.rects(D.layout(a)), D.pd(state).get("boxes")]))
    D.close(tab)


def disagreeing_snapshot(ws):
    tab, a, b, c, state = held_drop(ws, "coalesced", lambda r: D.edge_point(r, "right"))
    old = D.rects(D.layout(b))
    resize(b, "left")
    D.wait(lambda s: D.rects(D.layout(b)) != old, timeout=5)
    state = D.wait(lambda s: boxes_on_server(s, b), timeout=5)
    D.check("coalesced: the target's resize shows at once while the drop is pending",
            boxes_on_server(state, b) and D.pd(state).get("phase") == "dropping" and not D.settling(state),
            json.dumps([D.pd(state).get("phase"), active(state)]))
    D.hook("send-drop")
    resize(b, "left")
    state = D.wait(lambda s: D.idle(s) and boxes_on_server(s, b), timeout=3)
    final = D.rects(D.layout(b))
    D.check("coalesced: the drop ends on its reply, a placed right of b",
            D.idle(state) and a in final and final[a][0] > final[b][0], json.dumps([D.pd(state).get("phase"), final]))
    D.check("coalesced: the later snapshot wins, so the boxes end on the server's layout", boxes_on_server(state, b),
            json.dumps([D.pd(state).get("boxes"), D.host_rects(D.layout(b), state)]))
    D.close(tab)


def snapshot_before_reply(ws):
    tab, a, b, c, state = D.fresh(ws, "snapshot-first")
    before = dict(D.pd(state)["boxes"])
    D.hook("begin", pane=a)
    D.hook("move", steps=1, **D.centre(before[b]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=b))
    old = D.rects(D.layout(a))
    D.hook("hold-replies", on=True)
    D.hook("drop")
    D.wait(lambda s: D.rects(D.layout(a)) != old, timeout=5)
    state = D.wait(lambda s: boxes_on_server(s, a), timeout=5)
    D.check("snapshot-first: the swap's own snapshot shows at once, with no settle, the reply held",
            boxes_on_server(state, a) and D.pd(state).get("phase") == "dropping" and not D.settling(state),
            json.dumps([D.pd(state).get("phase"), active(state)]))
    D.motion("freeze", ms=0)
    D.hook("send-reply")
    D.hook("hold-replies", on=False)
    state = D.wait(lambda s: D.settling(s), timeout=5)
    got = D.pd(state).get("boxes") or {}
    D.check("snapshot-first: the reply settles a and b", {a, b} <= D.settling(state), json.dumps(active(state)))
    D.check("snapshot-first: the settle starts from the boxes at the release",
            all(p in got and D.near(got[p], before[p]) for p in (a, b)), json.dumps([got, before]))
    old = D.rects(D.layout(a))
    resize(a, "left")
    D.wait(lambda s: D.rects(D.layout(a)) != old, timeout=5)
    state = D.wait(lambda s: boxes_on_server(s, a), timeout=5)
    D.check("snapshot-first: a later snapshot during the settle applies at once, with no motion",
            boxes_on_server(state, a) and not D.settling(state), json.dumps([D.pd(state).get("boxes"), active(state)]))
    D.motion("run")
    state = D.wait(D.idle, timeout=5)
    D.check("snapshot-first: the drop ends idle on the server's layout", D.idle(state) and boxes_on_server(state, a),
            json.dumps([D.pd(state).get("phase"), D.pd(state).get("boxes")]))
    D.close(tab)


def tab_switch_pending(ws, other):
    tab, a, b, c, state = held_drop(ws, "tab-switch", lambda r: D.edge_point(r, "right"))
    S.cmd({"cmd": "select", "tab": other})
    state = D.wait(lambda s: s.get("selected_tab") == other and D.pd(s).get("phase") != "dropping", timeout=5)
    D.check("tab-switch: the pending drop ends quietly, with no cancel and no chip",
            D.pd(state).get("phase") == "idle" and "cancel" not in active(state)
            and not (D.pd(state).get("chip") or {}).get("visible"),
            json.dumps([state.get("selected_tab"), D.pd(state).get("phase"), active(state), D.pd(state).get("chip")]))
    shown = dict(D.pd(state).get("boxes") or {})
    D.hook("send-drop")
    D.wait(lambda s: a in D.rects(D.layout(b)) and D.rects(D.layout(b))[a][0] > D.rects(D.layout(b))[b][0], timeout=5)
    state = D.wait(D.idle, timeout=5)
    D.check("tab-switch: the late reply changes nothing on the new tab",
            D.idle(state) and state.get("selected_tab") == other and (D.pd(state).get("boxes") or {}) == shown,
            json.dumps([state.get("selected_tab"), D.pd(state).get("boxes"), shown]))
    D.close(tab)


def refused_drop(ws):
    tab, a, b, c, state = D.fresh(ws, "refused")
    D.motion("freeze", ms=0)
    D.hook("begin", pane=c)
    D.hook("move", **D.centre(D.pd(state)["boxes"][b]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=b))
    S.lab("herdr", "pane", "close", b)
    D.wait(lambda s: b not in (D.pd(s).get("boxes") or {}))
    closed = D.rects(D.layout(a))
    D.hook("drop")
    state = D.wait(lambda s: D.pd(s).get("phase") == "cancelling" or D.idle(s), timeout=5)
    got = D.changes(state)
    D.check("refused: the release sent the swap", [g["method"] for g in got] == ["pane.swap"], json.dumps(got))
    D.check("refused: the refusal plays the cancel", D.pd(state).get("phase") == "cancelling" and "cancel" in active(state),
            json.dumps([D.pd(state).get("phase"), active(state)]))
    D.motion("run")
    state = D.wait(D.idle, timeout=5)
    D.check("refused: the cancel ends idle", D.idle(state), json.dumps([D.pd(state).get("phase"), active(state)]))
    after = D.rects(D.layout(a))
    D.check("refused: nothing moved but the closed pane", set(after) == {a, c} and after == closed and boxes_on_server(state, a),
            json.dumps([closed, after, D.pd(state).get("boxes")]))
    D.close(tab)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = D.api("workspace", "list")["workspaces"]
    drag_ws = D.api("workspace", "create", "--label", "drag", "--no-focus")
    dest_ws = D.api("workspace", "create", "--label", "dest", "--no-focus")["workspace"]["workspace_id"]
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = drag_ws["workspace"]["workspace_id"]
    root = drag_ws["root_pane"]["pane_id"]
    probe = D.raw("pane", "place", root, "--beside", root, "--side", "right", "--dry-run")
    if (probe.get("result") or {}).get("place", {}).get("reason") != "same_pane":
        print(f"[BLOCKED] lab server does not answer pane.place: {json.dumps(probe)[:300]}")
        S.lab("down")
        raise SystemExit(2)
    tab2 = D.api("tab", "create", "--workspace", ws, "--label", "two", "--no-focus")["tab"]["tab_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})
    state = D.wait(lambda s: s.get("window_key") is True and "paneDrag" in s)
    if "paneDrag" not in state:
        D.check("the state dump has paneDrag", False)
        return D.finish()
    reduced_row_drop(ws, "tab:" + tab2, "reduced into-tab")
    reduced_row_drop(ws, "space:" + dest_ws, "reduced new-tab")
    refused_drop(ws)
    pending_foreign_resize(ws)
    refused_after_foreign(ws)
    disagreeing_snapshot(ws)
    snapshot_before_reply(ws)
    tab_switch_pending(ws, tab2)
    D.finish()


if __name__ == "__main__":
    main()
