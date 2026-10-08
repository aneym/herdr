#!/usr/bin/env python3
"""Drag a pane by its cap to rearrange the tab (pane drag slice S6; owner-written, do not edit).

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 macos/HerdrShell/scripts/check_pane_drag.py
The lab server is HERDR_SHELL_BIN and must answer `pane.place` (S1); the default is this
checkout's target/release or target/debug herdr, else ~/.local/bin/herdr. A server without
pane.place is BLOCKED (exit 2). `--spring` belongs to S8 and exits 3 until S8 writes it.

Mouse events go through NSApp.sendEvent from the TestHook, as check_pin_drag does, so the
app's own hit-testing decides cap, zone and row. Hook lines (JSON on the control FIFO):
  {"cmd":"pane-drag","op":"begin","pane":P,"travel":8}   press the centre of P's cap, drag `travel` pt right
  {"cmd":"pane-drag","op":"move","x":X,"y":Y,"steps":4}  drag to host point (PaneHostView, top-left origin)
  {"cmd":"pane-drag","op":"move","row":ROW_ID,"drop":true}  drag to a sidebar row's centre, release there
  {"cmd":"pane-drag","op":"drop"}                        release at the last point
  {"cmd":"pane-drag","op":"cancel","via":"esc"|"right"}  typed Esc, or a right click at the last point
  {"cmd":"pane-drag","op":"hold-drops","on":B} / {"op":"send-drop"}  hold the next drop's call unsent / send it
  {"cmd":"pane-drag","op":"hold-replies","on":B} / {"op":"send-reply"}  hold a drop's reply once in / handle it
  {"cmd":"motion","op":"freeze","ms":N} | {"cmd":"motion","op":"run"} | {"cmd":"motion","op":"reduce","on":true|false|null}
State dump key `paneDrag` (see the S6 brief): phase, source, zone, lastZone, ghostRect,
ghostTarget, ghostSource, dryRunPending, placeSupported, chip, boxes, sent, motion; plus
`surfaces[].mouse_sent`.

Scenario (spec S3 steps 1-7 on the Mac, plus FLIP, Reduce Motion and the keyboard chord):
  1. a drag from A's cap into B's right band sends one pane.place dry run; centre sends none;
     a revisit of the same zone reuses the answer; no surface gets a mouse event meanwhile;
  2. the ghost heads for the server's placed_rect and the chip carries A's cap name;
  3. the release sends pane.place {dry_run:false, focus:true} and the server agrees;
  4. Esc, a right click, or a release on the source sends no change and puts everything back;
     a press under the threshold is a click that focuses the pane;
  5. a centre release sends pane.swap and focus stays on the dragged pane;
  6. a release on a sidebar tab row sends pane.place {target: tab, side: right};
  7. a release on a space header sends pane.move {new_tab {workspace_id}};
  FLIP: C onto A's left edge settles three panes; frozen at 0/100/200 ms each pane box moves
  monotonically from its old host rect to its new one and every terminal surface already has
  its final size and grid; Reduce Motion puts the boxes at the new rects at 0 ms (crossfade);
  keyboard: cmd+opt+m lifts the focused pane, shift+arrow picks an edge, return drops, Esc cancels.
Writes macos/HerdrShell/checks/PANE-DRAG.txt, PANE-DRAG-flip.json and two shots.
"""
import json
import os
import pathlib
import sys
import time

if "--spring" in sys.argv:
    raise SystemExit(3)  # spring-loaded tabs: slice S8 adds this scenario
if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_pane_drag requires HERDR_SHELL_SPACE=1; host launch is forbidden")
# Short name: the Space bridge forwards sessions/<name>/herdr-client.sock and ssh refuses 104+ bytes.
os.environ["SHELL_LAB"] = "shellspike-pg"
ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
if not os.environ.get("HERDR_SHELL_BIN"):
    builds = [REPO / "target/release/herdr", REPO / "target/debug/herdr"]
    os.environ["HERDR_SHELL_BIN"] = str(next((b for b in builds if b.exists()), pathlib.Path.home() / ".local/bin/herdr"))
sys.path.insert(0, str(ROOT / "scripts"))
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/PANE-DRAG.txt")
TAB_EDGE = 12  # ShellMotion.tabEdgePx
lines, failures, flip_log = [], [], {}


def check(name, ok, detail=""):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" ({detail})" if detail and not ok else "")
    print(line, flush=True)
    lines.append(line)
    if not ok:
        failures.append(name)


def raw(*args):
    return json.loads(S.lab("herdr", *args) or "{}")


def api(*args):
    return raw(*args)["result"]


def wait(predicate, timeout=10):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.15)
    return state


def hook(op, **kw):
    S.cmd({"cmd": "pane-drag", "op": op, **kw})


def motion(op, **kw):
    S.cmd({"cmd": "motion", "op": op, **kw})


def pd(state):
    return state.get("paneDrag") or {}


def layout(pane):
    return api("pane", "layout", "--pane", pane)["layout"]


def rects(lay):
    return {p["pane_id"]: [p["rect"][k] for k in ("x", "y", "width", "height")] for p in lay["panes"]}


def host_rect(r, area, host_size):
    """A layout rect in host points, as PaneHostView.layout places a pane box (cap + body)."""
    sx, sy = host_size[0] / area["width"], host_size[1] / area["height"]
    gap_l = 1 if r[0] > area["x"] else 0
    gap_t = 1 if r[1] > area["y"] else 0
    return [(r[0] - area["x"]) * sx + gap_l, (r[1] - area["y"]) * sy + gap_t, r[2] * sx - gap_l, r[3] * sy - gap_t]


def host_rects(lay, state):
    return {p: host_rect(r, lay["area"], state["host_size"]) for p, r in rects(lay).items()}


def near(a, b, tol=2.0):
    return a is not None and b is not None and len(a) == len(b) and all(abs(x - y) <= tol for x, y in zip(a, b))


def centre(r):
    return {"x": r[0] + r[2] / 2, "y": r[1] + r[3] / 2}


def edge_point(r, side):
    """A point inside r's band on `side`, clear of the 12 pt tab edge."""
    length = r[2] if side in ("left", "right") else r[3]
    band = min(max(length * 0.25, 24), length * 0.33)
    d = (band + TAB_EDGE) / 2
    c = centre(r)
    return {"left": {"x": r[0] + d, "y": c["y"]}, "right": {"x": r[0] + r[2] - d, "y": c["y"]},
            "up": {"x": c["x"], "y": r[1] + d}, "down": {"x": c["x"], "y": r[1] + r[3] - d}}[side]


def sent(state):
    return pd(state).get("sent") or []


def dry_runs(state):
    return [c for c in sent(state) if c.get("method") == "pane.place" and c.get("params", {}).get("dry_run") is True]


def changes(state):
    """Calls that change the session: everything but pane.place dry runs and pane.focus."""
    return [c for c in sent(state) if c not in dry_runs(state) and c.get("method") != "pane.focus"]


def mouse_sent(state):
    return {s["pane"]: s.get("mouse_sent") for s in state.get("surfaces", [])}


def surfaces(state):
    return {s["pane"]: s for s in state.get("surfaces", [])}


def idle(state):
    d = pd(state)
    return d.get("phase") == "idle" and not (d.get("motion") or {}).get("active")


def settling(state):
    return {m.get("pane") for m in (pd(state).get("motion") or {}).get("active", []) if m.get("kind") == "settle"}


def zone_is(state, **want):
    z = pd(state).get("zone") or {}
    return all(z.get(k) == v for k, v in want.items())


def split(pane, direction):
    before = set(rects(layout(pane)))
    S.lab("herdr", "pane", "split", pane, "--direction", direction)
    return (set(rects(layout(pane))) - before).pop()


def fresh(ws, label):
    """A new tab A | (B / C), selected; returns (tab, a, b, c, state)."""
    made = api("tab", "create", "--workspace", ws, "--label", label, "--no-focus")
    tab, a = made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    b = split(a, "right")
    c = split(b, "down")
    S.lab("herdr", "pane", "rename", a, "drag-me")
    S.cmd({"cmd": "select", "tab": tab})
    state = wait(lambda s: s.get("selected_tab") == tab and set((pd(s).get("boxes") or {})) == {a, b, c}
                 and len(s.get("pane_caps", [])) == 3 and idle(s))
    return tab, a, b, c, state


def close(tab):
    S.lab("herdr", "tab", "close", tab)


def cancel_case(ws, name, how):
    tab, a, b, c, state = fresh(ws, "cancel-" + how)
    before = rects(layout(a))
    boxes0 = pd(state)["boxes"]
    hook("begin", pane=a)
    hook("move", **edge_point(boxes0[b], "right"))
    state = wait(lambda s: zone_is(s, kind="pane_edge", target=b, side="right") and not pd(s).get("dryRunPending"))
    check(f"{name}: the drag reaches B's right band first", zone_is(state, kind="pane_edge", target=b, side="right"),
          json.dumps(pd(state).get("zone")))
    if how in ("esc", "right"):
        hook("cancel", via=how)
        # The hook queues Esc as a typed key; release only once the cancel has landed.
        wait(lambda s: pd(s).get("phase") != "lifted", timeout=5)
        hook("drop")
    else:
        hook("move", **centre(boxes0[a]))
        state = S.state()
        check(f"{name}: over the source there is no zone and the pane stays lifted",
              pd(state).get("zone") is None and pd(state).get("phase") == "lifted", json.dumps(pd(state).get("zone")))
        hook("drop")
    state = wait(idle, timeout=5)
    time.sleep(0.3)
    state = S.state()
    check(f"{name}: no change is sent", changes(state) == [], json.dumps(changes(state)))
    check(f"{name}: the server layout is unchanged", rects(layout(a)) == before, json.dumps(rects(layout(a))))
    check(f"{name}: boxes return to their rects and the ghost clears",
          idle(state) and pd(state).get("ghostRect") is None
          and all(near(pd(state)["boxes"].get(p), boxes0[p]) for p in (a, b, c)), json.dumps(pd(state).get("boxes")))
    close(tab)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    drag_ws = api("workspace", "create", "--label", "drag", "--no-focus")
    dest_ws = api("workspace", "create", "--label", "dest", "--no-focus")["workspace"]["workspace_id"]
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = drag_ws["workspace"]["workspace_id"]
    probe = raw("pane", "place", drag_ws["root_pane"]["pane_id"], "--beside", drag_ws["root_pane"]["pane_id"],
                "--side", "right", "--dry-run")
    if (probe.get("result") or {}).get("place", {}).get("reason") != "same_pane":
        print(f"[BLOCKED] lab server {os.environ['HERDR_SHELL_BIN']} does not answer pane.place: {json.dumps(probe)[:300]}")
        S.lab("down")
        raise SystemExit(2)
    tab2 = api("tab", "create", "--workspace", ws, "--label", "two", "--no-focus")["tab"]["tab_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})  # synthesized mouse events need the key window; the Space is the app's own desktop
    state = wait(lambda s: s.get("window_key") is True and "paneDrag" in s)
    if "paneDrag" not in state:
        check("the state dump has paneDrag", False)
        return finish()
    check("pane.place is detected on the lab server", pd(state).get("placeSupported") is True, str(pd(state).get("placeSupported")))

    # Steps 1-3: pane edge with a dry run, ghost at placed_rect, drop.
    tab, a, b, c, state = fresh(ws, "edge")
    boxes0 = pd(state)["boxes"]
    dry = api("pane", "place", a, "--beside", b, "--side", "right", "--dry-run")["place"]
    want_ghost = host_rect([dry["placed_rect"][k] for k in ("x", "y", "width", "height")], dry["target_layout"]["area"], state["host_size"])
    mouse0 = mouse_sent(state)
    hook("begin", pane=a)
    state = S.state()
    check("1: past the threshold the pane lifts with no zone",
          pd(state).get("phase") == "lifted" and pd(state).get("source") == a and pd(state).get("zone") is None, json.dumps(pd(state))[:300])
    # One drag event straight to B's centre: a stepped path from A's cap crosses B's left band on the way.
    hook("move", steps=1, **centre(boxes0[b]))
    state = wait(lambda s: zone_is(s, kind="centre", target=b))
    check("1: over B's centre the zone is centre and no dry run is sent", zone_is(state, kind="centre", target=b) and dry_runs(state) == [],
          json.dumps([pd(state).get("zone"), dry_runs(state)]))
    check("1: the centre ghost is B's box", near(pd(state).get("ghostTarget"), boxes0[b]), f"{pd(state).get('ghostTarget')} vs {boxes0[b]}")
    hook("move", **edge_point(boxes0[b], "right"))
    state = wait(lambda s: zone_is(s, kind="pane_edge", target=b, side="right") and not pd(s).get("dryRunPending"))
    calls = dry_runs(state)
    check("1: B's right band sends exactly one pane.place dry run",
          len(calls) == 1 and calls[0]["params"].get("pane_id") == a and calls[0]["params"].get("target") == {"type": "pane", "pane_id": b}
          and calls[0]["params"].get("side") == "right", json.dumps(sent(state)))
    state = wait(lambda s: near(pd(s).get("ghostRect"), pd(s).get("ghostTarget"), 1.0))
    check("2: the ghost settles on the server's placed_rect",
          pd(state).get("ghostSource") == "placed" and near(pd(state).get("ghostTarget"), want_ghost)
          and near(pd(state).get("ghostRect"), want_ghost), f"{pd(state).get('ghostSource')} {pd(state).get('ghostRect')} want {want_ghost}")
    cap_name = next((cap["name"] for cap in state.get("pane_caps", []) if cap["id"] == a), None)
    chip = pd(state).get("chip") or {}
    check("2: the chip shows A's cap name", chip.get("visible") is True and chip.get("label") == cap_name and cap_name,
          f"{chip} cap={cap_name!r}")
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/PANE-DRAG-ghost.png")})
    hook("move", **centre(boxes0[b]))
    hook("move", **edge_point(boxes0[b], "right"))
    state = wait(lambda s: zone_is(s, kind="pane_edge", target=b, side="right") and not pd(s).get("dryRunPending"))
    check("1: returning to the same zone reuses the answer", len(dry_runs(state)) == 1, json.dumps(dry_runs(state)))
    check("1: no pane surface receives a mouse event during the drag", mouse_sent(state) == mouse0,
          f"{mouse0} -> {mouse_sent(state)}")
    hook("drop")
    state = wait(lambda s: idle(s) and changes(s))
    got = changes(state)
    params = got[0]["params"] if got else {}
    check("3: the release sends pane.place dry_run false, focus true",
          len(got) == 1 and got[0]["method"] == "pane.place" and params.get("pane_id") == a
          and params.get("target") == {"type": "pane", "pane_id": b} and params.get("side") == "right"
          and params.get("dry_run") is False and params.get("focus") is True, json.dumps(got))
    lay1 = layout(a)
    check("3: the server placed A where the dry run said, focused",
          rects(lay1).get(a) == [dry["placed_rect"][k] for k in ("x", "y", "width", "height")] and lay1["focused_pane_id"] == a,
          f"{rects(lay1)} focus={lay1['focused_pane_id']}")
    state = wait(lambda s: idle(s) and all(near(pd(s)["boxes"].get(p), r) for p, r in host_rects(lay1, s).items()))
    check("3: the boxes land on the server's layout", all(near(pd(state)["boxes"].get(p), r) for p, r in host_rects(lay1, state).items()),
          f"{pd(state).get('boxes')} want {host_rects(lay1, state)}")
    close(tab)

    # Step 4: cancel paths, and a press under the threshold.
    cancel_case(ws, "4 esc", "esc")
    cancel_case(ws, "4 right click", "right")
    cancel_case(ws, "4 release on the source", "source")
    tab, a, b, c, state = fresh(ws, "click")
    # A Shell click focuses the pane in the Shell (its key surface), as every Shell pane click does; it sends no
    # pane.focus, so the server's focused pane is not what this step reads.
    state = wait(lambda s: s.get("focused_pane") not in (None, b))
    check("4: precondition: another pane has focus before the click", state.get("focused_pane") not in (None, b),
          str(state.get("focused_pane")))
    hook("begin", pane=b, travel=2)
    state = S.state()
    check("4: a press under the threshold does not lift", pd(state).get("phase") in ("idle", "pressed"), str(pd(state).get("phase")))
    hook("drop")
    state = wait(lambda s: s.get("focused_pane") == b, timeout=5)
    check("4: the click focuses the pane and moves nothing", state.get("focused_pane") == b and changes(state) == [],
          f"focus={state.get('focused_pane')} {json.dumps(changes(state))}")
    close(tab)

    # Step 5: centre swap.
    tab, a, b, c, state = fresh(ws, "swap")
    before = rects(layout(a))
    hook("begin", pane=a)
    hook("move", steps=1, **centre(pd(state)["boxes"][b]))  # straight to the centre, as in step 1
    hook("drop")
    state = wait(lambda s: idle(s) and changes(s))
    got = changes(state)
    check("5: a centre release sends one pane.swap and no pane.place",
          [g["method"] for g in got] == ["pane.swap"] and got[0]["params"] == {"source_pane_id": a, "target_pane_id": b}
          and not any(x["method"] == "pane.place" for x in sent(state)), json.dumps(sent(state)))
    after = layout(a)
    check("5: the server swapped them and focus stays on the dragged pane",
          rects(after).get(a) == before[b] and rects(after).get(b) == before[a] and after["focused_pane_id"] == a,
          f"{rects(after)} focus={after['focused_pane_id']}")
    close(tab)

    # Step 6: a sidebar tab row.
    tab, a, b, c, state = fresh(ws, "into-tab")
    state = wait(lambda s: any(r.split("|")[1] == "tab:" + tab2 for r in s.get("spaces_rows", [])))
    hook("begin", pane=a)
    hook("move", row="tab:" + tab2, drop=True)
    state = wait(lambda s: idle(s) and changes(s))
    got = changes(state)
    params = got[0]["params"] if got else {}
    check("6: a release on a tab row sends pane.place target tab, side right",
          (pd(state).get("lastZone") or {}).get("kind") == "into_tab" and len(got) == 1 and got[0]["method"] == "pane.place"
          and params.get("target") == {"type": "tab", "tab_id": tab2} and params.get("side") == "right"
          and params.get("dry_run") is False and params.get("focus") is True, json.dumps([pd(state).get("lastZone"), got]))
    moved = layout(a)
    r = rects(moved).get(a)
    check("6: the server put A in that tab as a full-height right third",
          moved["tab_id"] == tab2 and r is not None and r[1] == moved["area"]["y"] and r[3] == moved["area"]["height"]
          and abs(r[2] - moved["area"]["width"] / 3) <= 1, f"{moved['tab_id']} {r} area={moved['area']}")
    close(tab)

    # Step 7: a space header.
    tab, a, b, c, state = fresh(ws, "new-tab")
    tabs_before = len(api("tab", "list", "--workspace", dest_ws)["tabs"])
    state = wait(lambda s: any(r.split("|")[1] == "space:" + dest_ws for r in s.get("spaces_rows", [])))
    hook("begin", pane=a)
    hook("move", row="space:" + dest_ws, drop=True)
    state = wait(lambda s: idle(s) and changes(s))
    got = changes(state)
    check("7: a release on a space header sends pane.move new_tab in that space",
          (pd(state).get("lastZone") or {}).get("kind") == "new_tab_in" and len(got) == 1 and got[0]["method"] == "pane.move"
          and got[0]["params"].get("pane_id") == a and got[0]["params"].get("destination") == {"type": "new_tab", "workspace_id": dest_ws}
          and got[0]["params"].get("focus") is True,
          json.dumps([pd(state).get("lastZone"), got]))
    check("7: the server opened it as a new tab there and the source tab kept two panes",
          len(api("tab", "list", "--workspace", dest_ws)["tabs"]) == tabs_before + 1 and set(rects(layout(b))) == {b, c},
          f"{tabs_before} -> {len(api('tab', 'list', '--workspace', dest_ws)['tabs'])}; {set(rects(layout(b)))}")
    close(tab)

    flip(ws)
    reduced(ws)
    keyboard(ws)
    finish()


def flip(ws):
    """C onto A's left edge: (C | A) | B, so A, B and C all settle."""
    tab, a, b, c, state = fresh(ws, "flip")
    old = {p: pd(state)["boxes"][p] for p in (a, b, c)}
    motion("freeze", ms=0)
    hook("begin", pane=c)
    hook("move", **edge_point(old[a], "left"))
    wait(lambda s: zone_is(s, kind="pane_edge", target=a, side="left") and not pd(s).get("dryRunPending"))
    hook("drop")
    deadline = time.monotonic() + 10
    lay = layout(a)
    while rects(lay)[b][3] != lay["area"]["height"] and time.monotonic() < deadline:
        time.sleep(0.2)
        lay = layout(a)
    state = wait(lambda s: settling(s) >= {a, b, c})
    new = host_rects(lay, state)
    samples = {}
    for ms in (0, 100, 200):
        motion("freeze", ms=ms)
        s = S.state()
        samples[ms] = {"boxes": {p: pd(s).get("boxes", {}).get(p) for p in (a, b, c)},
                       "surfaces": {p: surfaces(s).get(p, {}) for p in (a, b, c)},
                       "motion": pd(s).get("motion")}
        if ms == 100:
            S.cmd({"cmd": "shot", "out": str(ROOT / "checks/PANE-DRAG-settle-100.png")})
    motion("run")
    final = wait(idle, timeout=5)
    flip_log.update({"old": old, "new": new, "samples": samples, "final": pd(final).get("boxes")})
    check("FLIP: A, B and C settle after the drop", settling({"paneDrag": {"motion": samples[0]["motion"]}}) >= {a, b, c},
          json.dumps(samples[0]["motion"]))
    for p, name in ((a, "A"), (b, "B"), (c, "C")):
        v = [samples[ms]["boxes"][p] or [0, 0, 0, 0] for ms in (0, 100, 200)]
        o, n = old[p], new[p]
        starts = near(v[0], o, 1.5)
        dist = [[abs(x[i] - n[i]) for i in range(4)] for x in v]
        monotonic = all(dist[k + 1][i] <= dist[k][i] + 0.5 for k in range(2) for i in range(4))
        bounded = all(min(o[i], n[i]) - 0.5 <= x[i] <= max(o[i], n[i]) + 0.5 for x in v for i in range(4))
        moving = [i for i in range(4) if abs(o[i] - n[i]) > 4]
        progress = bool(moving) and all(dist[1][i] < dist[0][i] - 1 for i in moving)
        check(f"FLIP {name}: the box starts at its old rect and moves monotonically toward the new one",
              starts and monotonic and bounded and progress, f"old {o} new {n} samples {v}")
        check(f"FLIP {name}: the box ends at the new rect", near((pd(final).get("boxes") or {}).get(p), n),
              f"{(pd(final).get('boxes') or {}).get(p)} want {n}")
        end = surfaces(final).get(p, {})
        sizes = [samples[ms]["surfaces"][p] for ms in (0, 100, 200)]
        same = all(s.get("frame") and end.get("frame") and near(s["frame"][2:], end["frame"][2:], 0.5)
                   and (s.get("cols"), s.get("rows")) == (end.get("cols"), end.get("rows")) for s in sizes)
        check(f"FLIP {name}: the terminal surface has its final size and grid throughout the settle", same,
              f"{[(s.get('frame'), s.get('cols'), s.get('rows')) for s in sizes]} final {(end.get('frame'), end.get('cols'), end.get('rows'))}")
    (ROOT / "checks/PANE-DRAG-flip.json").write_text(json.dumps(flip_log, indent=2) + "\n")
    close(tab)


def reduced(ws):
    tab, a, b, c, state = fresh(ws, "reduce")
    motion("reduce", on=True)
    old = pd(state)["boxes"]
    motion("freeze", ms=0)
    hook("begin", pane=c)
    hook("move", **edge_point(old[a], "left"))
    wait(lambda s: zone_is(s, kind="pane_edge", target=a, side="left") and not pd(s).get("dryRunPending"))
    hook("drop")
    state = wait(lambda s: any(m.get("kind") == "crossfade" for m in (pd(s).get("motion") or {}).get("active", [])))
    new = host_rects(layout(a), state)
    kinds = {m.get("kind") for m in (pd(state).get("motion") or {}).get("active", []) if m.get("pane")}
    check("Reduce Motion: the drop crossfades instead of translating", kinds == {"crossfade"}, str(kinds))
    check("Reduce Motion: the boxes are at their new rects at 0 ms",
          all(near(pd(state).get("boxes", {}).get(p), new[p]) for p in (a, b, c)), f"{pd(state).get('boxes')} want {new}")
    motion("run")
    motion("reduce", on=None)
    wait(idle, timeout=5)
    close(tab)


def keyboard(ws):
    tab, a, b, c, state = fresh(ws, "keys")
    S.lab("herdr", "pane", "focus", a)
    wait(lambda s: s.get("focused_pane") == a)
    right = api("pane", "neighbor", "--pane", a, "--direction", "right")["neighbor"].get("neighbor_pane_id")
    S.key("m", ("cmd", "opt"))
    state = wait(lambda s: pd(s).get("phase") == "lifted")
    check("keys: cmd+opt+m lifts the focused pane with its right neighbour as a centre target",
          pd(state).get("source") == a and zone_is(state, kind="centre", target=right), json.dumps([pd(state).get("source"), pd(state).get("zone")]))
    S.key("right", ("shift",))
    state = wait(lambda s: zone_is(s, kind="pane_edge", target=right, side="right") and not pd(s).get("dryRunPending"))
    check("keys: shift+right picks the target's right edge and sends one dry run",
          zone_is(state, kind="pane_edge", target=right, side="right") and len(dry_runs(state)) == 1, json.dumps(sent(state)))
    S.key("return")
    state = wait(lambda s: idle(s) and changes(s))
    got = changes(state)
    check("keys: return drops with pane.place",
          len(got) == 1 and got[0]["method"] == "pane.place" and got[0]["params"].get("target") == {"type": "pane", "pane_id": right}
          and got[0]["params"].get("side") == "right" and got[0]["params"].get("dry_run") is False, json.dumps(got))
    close(tab)
    tab, a, b, c, state = fresh(ws, "keys-esc")
    before = rects(layout(a))
    S.lab("herdr", "pane", "focus", a)
    wait(lambda s: s.get("focused_pane") == a)
    S.key("m", ("cmd", "opt"))
    state = wait(lambda s: pd(s).get("phase") == "lifted")
    check("keys: cmd+opt+m lifts before escape", pd(state).get("phase") == "lifted", str(pd(state).get("phase")))
    S.key("escape")
    state = wait(idle, timeout=5)
    check("keys: escape cancels with nothing changed", changes(state) == [] and rects(layout(a)) == before, json.dumps(changes(state)))
    close(tab)


def finish():
    S.app("stop")
    S.lab("down")
    pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
