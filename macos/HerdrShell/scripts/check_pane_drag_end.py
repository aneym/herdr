#!/usr/bin/env python3
"""Every pane drop ends (pane drag fix S6b): regressions beside check_pane_drag.py, on its helpers.

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 macos/HerdrShell/scripts/check_pane_drag_end.py
Same lab server rules as check_pane_drag.py (HERDR_SHELL_BIN must answer pane.place; else BLOCKED, exit 2).

  1. Reduce Motion, a release on a sidebar tab row: the source leaves the tab, so the layout brings
     no crossfade; the drop still ends (phase idle, no motion) instead of sticking in settling.
  2. The same for a release on a space header (a new tab in that space).
  3. A drop the server refuses (a centre swap with a pane closed under the drag) plays the cancel:
     the phase passes through cancelling with a cancel motion, then ends idle with the layout as it was.
S10/S10b (a drop ends with the server's reply, never with a guess from snapshots; while it is pending the boxes stay
where they were at the release and snapshots are buffered). The hook holds the drop's call unsent, as a slow link
would, while other clients change the tab:
  4. An edge drop pending while another client resizes the source: the boxes stay frozen, the drop stays pending;
     the reply then settles the panes from the release boxes onto the server's layout.
  5. A swap refused after other clients resized the source and closed the target: the boxes stay frozen until the
     refusal, which shows the buffered snapshot at once and plays the cancel.
  6. The target resized while the drop is pending and again right after it lands: the boxes stay frozen, the drop
     ends on its reply and the boxes end on the server's layout.
The hook also holds a reply that is in until the check releases it, so the drop's own snapshot lands first:
  7. A swap whose snapshot lands before its reply: the boxes stay frozen and the only call sent is pane.swap (the
     server focuses the source). Sampled on a frozen clock at 0/50/100/150/200 ms, the reply's settle starts at the
     release boxes and moves a and b monotonically to the server's layout: no frame shows the final layout first
     and nothing moves backwards. A snapshot during the settle retargets it from where the boxes are.
  8. A swap whose call was sent and whose reply is held, then another tab selected: the drop ends quietly (no
     cancel, no chip). The held reply is then delivered and ignored: selection, boxes, Shell focus, the server's
     layout and focus, and the calls sent are all unchanged.
Writes macos/HerdrShell/checks/PANE-DRAG-END.txt.
"""
import json
import pathlib
import sys
import time

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


def frozen_through(state, release, label):
    """The snapshot landed (the server moved) and the Shell still draws the release boxes, the drop pending."""
    got = D.pd(state).get("boxes") or {}
    D.check(f"{label}: the boxes stay where they were at the release while the drop is pending",
            set(got) == set(release) and all(D.near(got[p], release[p]) for p in release)
            and D.pd(state).get("frozen") is True and D.pd(state).get("phase") == "dropping" and not D.settling(state),
            json.dumps([got, release, D.pd(state).get("phase"), D.pd(state).get("frozen"), active(state)]))


def server_moved(pane, old):
    D.wait(lambda s: D.rects(D.layout(pane)) != old, timeout=5)
    time.sleep(0.6)  # the snapshot reaches the Shell
    return S.state()


def pending_foreign_resize(ws):
    tab, a, b, c, state = held_drop(ws, "pending-resize", lambda r: D.edge_point(r, "right"))
    release = dict(D.pd(state)["boxes"])
    old = D.rects(D.layout(a))
    resize(a, "left")
    frozen_through(server_moved(a, old), release, "pending")
    D.motion("freeze", ms=0)
    D.hook("send-drop")
    state = D.wait(lambda s: D.settling(s), timeout=5)
    got = D.pd(state).get("boxes") or {}
    D.check("pending: the reply settles the panes from the release boxes", {a, b} <= D.settling(state)
            and all(D.near(got.get(p), release[p]) for p in (a, b)), json.dumps([active(state), got, release]))
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
    release = dict(D.pd(state)["boxes"])
    old = D.rects(D.layout(a))
    resize(a, "left")
    frozen_through(server_moved(a, old), release, "refused-foreign")
    S.lab("herdr", "pane", "close", b)
    D.wait(lambda s: b not in D.rects(D.layout(a)), timeout=5)
    time.sleep(0.6)
    frozen_through(S.state(), release, "refused-foreign, target closed")
    closed = D.rects(D.layout(a))
    D.motion("freeze", ms=0)
    D.hook("send-drop")
    state = D.wait(lambda s: D.pd(s).get("phase") != "dropping", timeout=5)
    D.check("refused-foreign: the refusal shows the buffered snapshot at once and plays the cancel",
            D.pd(state).get("phase") == "cancelling" and "cancel" in active(state) and not D.settling(state)
            and boxes_on_server(state, a), json.dumps([D.pd(state).get("phase"), active(state), D.pd(state).get("boxes")]))
    D.motion("run")
    state = D.wait(D.idle, timeout=5)
    D.check("refused-foreign: nothing moved; the boxes stay on the server's layout",
            D.idle(state) and D.rects(D.layout(a)) == closed and boxes_on_server(state, a),
            json.dumps([closed, D.rects(D.layout(a)), D.pd(state).get("boxes")]))
    D.close(tab)


def disagreeing_snapshot(ws):
    tab, a, b, c, state = held_drop(ws, "coalesced", lambda r: D.edge_point(r, "right"))
    release = dict(D.pd(state)["boxes"])
    old = D.rects(D.layout(b))
    resize(b, "left")
    frozen_through(server_moved(b, old), release, "coalesced")
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
    state = server_moved(a, old)
    frozen_through(state, before, "snapshot-first")
    D.check("snapshot-first: the release sends pane.swap and no pane.focus",
            [x.get("method") for x in D.sent(state)] == ["pane.swap"], json.dumps(D.sent(state)))
    final = D.host_rects(D.layout(a), state)
    D.motion("freeze", ms=0)
    D.hook("send-reply")
    D.hook("hold-replies", on=False)
    state = D.wait(lambda s: D.settling(s), timeout=5)
    D.check("snapshot-first: the reply settles a and b", {a, b} <= D.settling(state), json.dumps(active(state)))
    samples = {}
    for ms in (0, 50, 100, 150, 200):
        D.motion("freeze", ms=ms)
        samples[ms] = {p: (D.pd(S.state()).get("boxes") or {}).get(p) for p in (a, b)}
    for p in (a, b):
        v = [samples[ms][p] or [0, 0, 0, 0] for ms in (0, 50, 100, 150, 200)]
        o, n = before[p], final[p]
        dist = [[abs(x[i] - n[i]) for i in range(4)] for x in v]
        starts = D.near(v[0], o, 1.5)
        forward = all(dist[k + 1][i] <= dist[k][i] + 0.5 for k in range(4) for i in range(4))
        bounded = all(min(o[i], n[i]) - 0.5 <= x[i] <= max(o[i], n[i]) + 0.5 for x in v for i in range(4))
        ends = D.near(v[-1], n)
        D.check(f"snapshot-first: {p} starts at its release box and only moves toward the final one",
                starts and forward and bounded and ends, json.dumps({"release": o, "final": n, "samples": v}))
    # A snapshot mid-settle retargets the running settle from where the boxes are; it never restarts from the release.
    D.motion("freeze", ms=100)
    mid = dict(D.pd(S.state()).get("boxes") or {})
    old = D.rects(D.layout(a))
    resize(a, "left")
    state = server_moved(a, old)
    target = D.host_rects(D.layout(a), state)
    got = D.pd(state).get("boxes") or {}
    between = all(min(mid[a][i], target[a][i]) - 0.5 <= got[a][i] <= max(mid[a][i], target[a][i]) + 0.5 for i in range(4))
    D.check("snapshot-first: a snapshot during the settle retargets it from the drawn boxes",
            a in D.settling(state) and between and not D.near(got[a], before[a]),
            json.dumps([active(state), mid.get(a), got.get(a), target.get(a)]))
    D.motion("run")
    state = D.wait(lambda s: D.idle(s) and boxes_on_server(s, a), timeout=5)
    D.check("snapshot-first: the drop ends idle on the server's layout", D.idle(state) and boxes_on_server(state, a),
            json.dumps([D.pd(state).get("phase"), D.pd(state).get("boxes")]))
    D.close(tab)


def tab_switch_pending(ws, other, other_pane):
    tab, a, b, c, state = D.fresh(ws, "tab-switch")
    D.hook("begin", pane=a)
    D.hook("move", steps=1, **D.centre(D.pd(state)["boxes"][b]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=b))
    old = D.rects(D.layout(a))
    D.hook("hold-replies", on=True)
    D.hook("drop")
    # The call went out and the server applied it; its reply is in and held.
    D.wait(lambda s: D.rects(D.layout(a)) != old, timeout=5)
    state = D.wait(lambda s: D.pd(s).get("replyHeld") is True, timeout=5)
    D.check("tab-switch: the swap was sent and its reply is held", D.pd(state).get("replyHeld") is True
            and [x.get("method") for x in D.sent(state)] == ["pane.swap"], json.dumps([D.pd(state).get("replyHeld"), D.sent(state)]))
    S.cmd({"cmd": "select", "tab": other})
    state = D.wait(lambda s: s.get("selected_tab") == other and D.pd(s).get("phase") != "dropping", timeout=5)
    D.check("tab-switch: the pending drop ends quietly, with no cancel and no chip",
            D.pd(state).get("phase") == "idle" and "cancel" not in active(state)
            and not (D.pd(state).get("chip") or {}).get("visible"),
            json.dumps([state.get("selected_tab"), D.pd(state).get("phase"), active(state), D.pd(state).get("chip")]))
    state = D.wait(lambda s: D.idle(s) and s.get("focused_pane") is not None, timeout=5)
    replies = dict(D.pd(state).get("replies") or {})
    shown = (dict(D.pd(state).get("boxes") or {}), state.get("focused_pane"), list(D.sent(state)))
    server = (D.layout(a), D.layout(other_pane))
    D.hook("send-reply")
    D.hook("hold-replies", on=False)
    state = D.wait(lambda s: (D.pd(s).get("replies") or {}).get("ignored", 0) > replies.get("ignored", 0), timeout=5)
    time.sleep(0.6)  # anything the reply could have set off has landed
    state = S.state()
    got = D.pd(state).get("replies") or {}
    D.check("tab-switch: the held reply was delivered after the switch and ignored",
            got.get("handled") == replies.get("handled", -1) + 1 and got.get("ignored") == replies.get("ignored", -1) + 1
            and D.pd(state).get("replyHeld") is False, json.dumps([replies, got, D.pd(state).get("replyHeld")]))
    now = (dict(D.pd(state).get("boxes") or {}), state.get("focused_pane"), list(D.sent(state)))
    D.check("tab-switch: the late reply changes no selection, box, Shell focus or call",
            D.idle(state) and state.get("selected_tab") == other and now == shown, json.dumps([state.get("selected_tab"), now, shown]))
    after = (D.layout(a), D.layout(other_pane))
    D.check("tab-switch: the late reply changes neither tab's layout or focus on the server",
            all(D.rects(x) == D.rects(y) and x.get("focused_pane_id") == y.get("focused_pane_id") for x, y in zip(server, after)),
            json.dumps([[D.rects(x), x.get("focused_pane_id")] for x in server], ) + " -> " + json.dumps([[D.rects(x), x.get("focused_pane_id")] for x in after]))
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
    made2 = D.api("tab", "create", "--workspace", ws, "--label", "two", "--no-focus")
    tab2, pane2 = made2["tab"]["tab_id"], made2["root_pane"]["pane_id"]
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
    tab_switch_pending(ws, tab2, pane2)
    D.finish()


if __name__ == "__main__":
    main()
