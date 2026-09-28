#!/usr/bin/env python3
"""P9 check: splits and pane commands.

  SHELL_LAB=shellspike-p9 python3 scripts/check_p9.py --out checks/P9.txt

Fresh lab -> app -> real mouse events (NSEvents through window.sendEvent, so hit-testing
picks the divider handle) drag the divider of the two-pane "shell spike" tab. Verified from
herdr itself (`herdr api snapshot` layouts) and from the app (each Ghostty surface's grid):
  1. the drag changes the herdr layout rect of BOTH panes, in the drag direction, and the
     split ratio matches the distance dragged;
  2. both surfaces report the new grid size (proportional to the new rects);
  3. dragging back the other way works (second-pane, opposite-direction path);
  4. split (via herdr's API) adds a pane and a divider; a horizontal divider drags too;
  5. new tab (via herdr's API) appears in herdr, is selected, and its surface renders.
The layout is always rebuilt from herdr's layout: the app's shown layout must equal herdr's.
"""
import json
import os
import subprocess
import sys
import time

os.environ.setdefault("SHELL_LAB", "shellspike-p9")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402  (helpers: lab, app, cmd, state, herdr_json)

_state = S.state


def _state_retry():
    """The host can be heavily loaded (Studio runs hot); give the app several tries."""
    for _ in range(12):
        try:
            return _state()
        except SystemExit:
            time.sleep(0.5)
    return _state()


S.state = _state_retry

lines, failures = [], []
OUT = os.path.join(S.D, "checks", "P9.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
SHOT = os.path.splitext(OUT)[0] + ".png"


def say(s=""):
    print(s)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def wait_for(pred, timeout=8.0, step=0.05):
    t0 = time.time()
    while time.time() - t0 < timeout:
        v = pred()
        if v:
            return v, time.time() - t0
        time.sleep(step)
    return None, None


def herdr_layout(tab):
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    lay = next((l for l in snap["layouts"] if l["tab_id"] == tab), None)
    return lay, snap


def rects(lay):
    return {p["pane_id"]: p["rect"] for p in lay["panes"]}


def surf(st):
    return {x["pane"]: x for x in st["surfaces"] if x["in_host"]}


def app_layout_matches_herdr(st, lay):
    """The layout the app shows must equal herdr's (rect for rect)."""
    shown = st["shown_layout"]
    if not shown or shown["tab"] != lay["tab_id"]:
        return False
    mine = {p["pane"]: p["rect"] for p in shown["panes"]}
    theirs = rects(lay)
    return set(mine) == set(theirs) and all(
        mine[k] == [theirs[k]["x"], theirs[k]["y"], theirs[k]["width"], theirs[k]["height"]] for k in theirs)


def drag(index, dx, steps=10):
    S.cmd({"cmd": "drag_divider", "index": index, "dx": dx, "steps": steps, "interval": 0.04})
    time.sleep(0.15)
    ok, _ = wait_for(lambda: not S.state()["drag_running"], timeout=15)
    return ok is not None


def main():
    say(f"HerdrShell P9 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    say(f"herdr: {S.lab('herdr', '--version').strip()} (lab binary)")
    say(f"host load average: {os.getloadavg()[0]:.0f} on {os.cpu_count()} cores")
    lay0, snap = herdr_layout(next(t["tab_id"] for t in S.herdr_json("api", "snapshot")["result"]["snapshot"]["tabs"]
                                   if t["label"] == "shell spike"))
    tab = lay0["tab_id"]
    p1, p2 = [p["pane_id"] for p in sorted(lay0["panes"], key=lambda p: p["rect"]["x"])]
    say(f"tab {tab}: pane1={p1} pane2={p2}; herdr layout area {lay0['area']['width']}x{lay0['area']['height']}")

    say(f"app start: {S.app('start').strip()}")

    def ready():
        st = S.state()
        s = surf(st)
        if st["selected_tab"] == tab and all(p in s and any("%" in l for l in s[p]["visible_nonblank"]) for p in (p1, p2)) \
                and len(st["dividers"]) == 1:
            return st
        return None
    st, _ = wait_for(ready, timeout=30)
    check("both panes attached and rendered, one divider handle over the split", st is not None)
    if st is None:
        return finish()
    s0 = surf(st)
    c1, c2 = s0[p1]["cols"], s0[p2]["cols"]
    r0 = rects(lay0)
    ratio0 = st["dividers"][0]["ratio"]
    host_w = float(st["host_frame"].replace("{", "").replace("}", "").split(",")[2])
    say(f"before: herdr widths {r0[p1]['width']:.0f}|{r0[p2]['width']:.0f}, ratio {ratio0:.3f}; surface grids {c1}|{c2} cols; host width {host_w:.0f} pt")
    check("app layout equals herdr layout before the drag", app_layout_matches_herdr(st, lay0))

    # 1 + 2. Drag right.
    dx = 200
    t0 = time.time()
    check("drag right finished", drag(0, dx))
    dt = time.time() - t0
    lay1, _ = herdr_layout(tab)
    r1 = rects(lay1)
    ratio1 = lay1["splits"][0]["ratio"]
    exp = ratio0 + dx / host_w
    say(f"after drag +{dx} pt ({dt:.2f}s): herdr widths {r1[p1]['width']:.0f}|{r1[p2]['width']:.0f}, ratio {ratio1:.3f} (expected about {exp:.3f})")
    check("herdr layout rect changed for pane 1 (wider)", r1[p1]["width"] > r0[p1]["width"] + 5,
          f"{r0[p1]['width']:.0f} -> {r1[p1]['width']:.0f}")
    check("herdr layout rect changed for pane 2 (narrower, moved right)",
          r1[p2]["width"] < r0[p2]["width"] - 5 and r1[p2]["x"] > r0[p2]["x"] + 5,
          f"w {r0[p2]['width']:.0f} -> {r1[p2]['width']:.0f}, x {r0[p2]['x']:.0f} -> {r1[p2]['x']:.0f}")
    check("split ratio matches the distance dragged", abs(ratio1 - exp) < 0.03, f"{ratio1:.3f} vs {exp:.3f}")

    def grids_follow():
        st = S.state()
        s = surf(st)
        a, b = s[p1]["cols"], s[p2]["cols"]
        frac = a / max(1, a + b)
        want = r1[p1]["width"] / (r1[p1]["width"] + r1[p2]["width"])
        if a > c1 + 5 and b < c2 - 5 and abs(frac - want) < 0.04:
            return st
        return None
    st, _ = wait_for(grids_follow, timeout=8)
    s1 = surf(S.state())
    say(f"surface grids after: {s1[p1]['cols']}x{s1[p1]['rows']} | {s1[p2]['cols']}x{s1[p2]['rows']} (before {c1}|{c2} cols)")
    check("both surfaces report the new grid size (pane 1 wider, pane 2 narrower, proportional to herdr's rects)",
          st is not None)
    st = S.state()
    check("app layout equals herdr layout after the drag (rebuilt from herdr)", app_layout_matches_herdr(st, lay1))
    say(f"resize requests sent: {st['resize_requests']}")

    # The PTY really has the new size: the shell in pane 1 (focused) reports it.
    S.type_("stty size")
    S.key("return")
    want_cols = str(s1[p1]["cols"])
    txt, dt = S.wait_read(p1, lambda x: any(l.split()[-1:] == [want_cols] and len(l.split()) == 2 and l.split()[0].isdigit()
                                            for l in x.splitlines()), timeout=5)
    check("pane 1's shell sees the new width (stty size == surface grid)", dt is not None,
          f"stty cols {want_cols}" if dt is not None else txt[-200:])

    # 3. Drag back left (second-pane / opposite-direction path).
    check("drag left finished", drag(0, -dx))
    lay2, _ = herdr_layout(tab)
    ratio2 = lay2["splits"][0]["ratio"]
    check("dragging back restores the ratio", abs(ratio2 - ratio0) < 0.03, f"{ratio2:.3f} vs {ratio0:.3f}")
    wait_for(lambda: app_layout_matches_herdr(S.state(), lay2), timeout=5)
    check("app layout equals herdr layout after dragging back", app_layout_matches_herdr(S.state(), lay2))

    # Clamp: a drag far past the edge must not loop or hang.
    t0 = time.time()
    check("drag far past the edge finishes (clamped, no request loop)", drag(0, 5000, steps=4))
    lay3, _ = herdr_layout(tab)
    check("ratio clamped at herdr's 0.9 limit", abs(lay3["splits"][0]["ratio"] - 0.9) < 0.011,
          f"{lay3['splits'][0]['ratio']:.3f}")
    check("drag back from the clamp", drag(0, -5000, steps=4))
    lay3b, _ = herdr_layout(tab)
    check("ratio clamped at herdr's 0.1 limit", abs(lay3b["splits"][0]["ratio"] - 0.1) < 0.011,
          f"{lay3b['splits'][0]['ratio']:.3f}")
    S.cmd({"cmd": "drag_divider", "index": 0, "dx": 0, "steps": 1})  # no-op click on the handle
    time.sleep(0.5)
    drag(0, int(host_w * 0.4))  # back near the middle for the split tests
    time.sleep(0.3)

    # 4. Split through the API (target: the focused pane).
    st = S.state()
    focused = st["focused_pane"]
    n_before = len(herdr_layout(tab)[0]["panes"])
    S.cmd({"cmd": "split", "direction": "right"})

    def split_done():
        lay, _ = herdr_layout(tab)
        st = S.state()
        s = surf(st)
        if len(lay["panes"]) == n_before + 1 and len(s) == n_before + 1 and len(st["dividers"]) == 2 \
                and all(any("%" in l for l in x["visible_nonblank"]) for x in s.values()) \
                and app_layout_matches_herdr(st, lay):
            return lay, st
        return None
    got, dt = wait_for(split_done, timeout=10)
    check("split right (herdr API) added a pane: 3 surfaces rendered, 2 dividers, app layout == herdr layout",
          got is not None, f"{dt:.2f}s" if dt else "")
    if got:
        lay4, st = got
        new_pane = next(p for p in rects(lay4) if p not in (p1, p2))
        check("the new pane took keyboard focus", wait_for(lambda: S.state()["focused_pane"] == new_pane, 3)[0] is not None,
              f"focused={S.state()['focused_pane']} new={new_pane} (split target was {focused})")
        S.cmd({"cmd": "split", "direction": "down"})

        def down_done():
            lay, _ = herdr_layout(tab)
            st = S.state()
            s = surf(st)
            if len(lay["panes"]) == 4 and len(s) == 4 and len(st["dividers"]) == 3 and app_layout_matches_herdr(st, lay) \
                    and all(any("%" in l for l in x["visible_nonblank"]) for x in s.values()):
                return lay, st
            return None
        got, dt = wait_for(down_done, timeout=10)
        check("split down (herdr API) added a fourth pane and a horizontal divider", got is not None,
              f"{dt:.2f}s" if dt else "")
        if got:
            lay5, st = got
            hi = next(i for i, d in enumerate(st["dividers"]) if not d["vertical"])
            hs = next(d for d in lay5["splits"] if d["direction"] == "down")
            top, bottom = st["dividers"][hi]["first_pane"], st["dividers"][hi]["second_pane"]
            rr = rects(lay5)
            hh0 = (rr[top]["height"], rr[bottom]["height"])
            h_ratio0 = hs["ratio"]
            dy = 100
            check("horizontal divider drag finished", drag(hi, dy))
            lay6, _ = herdr_layout(tab)
            rr6 = rects(lay6)
            hs6 = next(d for d in lay6["splits"] if d["direction"] == "down")
            say(f"horizontal divider: heights {hh0[0]:.0f}|{hh0[1]:.0f} -> {rr6[top]['height']:.0f}|{rr6[bottom]['height']:.0f}, ratio {h_ratio0:.3f} -> {hs6['ratio']:.3f}")
            check("horizontal drag changed both panes' herdr rects (top taller, bottom shorter)",
                  rr6[top]["height"] > hh0[0] + 2 and rr6[bottom]["height"] < hh0[1] - 2)
            wait_for(lambda: app_layout_matches_herdr(S.state(), lay6), timeout=5)
            st = S.state()
            check("app layout equals herdr layout after the horizontal drag", app_layout_matches_herdr(st, lay6))
            sg = surf(st)
            check("top and bottom surfaces report new grid rows",
                  sg[top]["rows"] > sg[bottom]["rows"] or rr6[top]["height"] <= rr6[bottom]["height"],
                  f"{top} {sg[top]['rows']} rows, {bottom} {sg[bottom]['rows']} rows")
            # A divider drag is a mouse gesture: it must not have sent anything into a pane.
            leaked = [p for p in (p1, p2) if "^[[<" in S.pane_read(p)]
            check("drags sent no mouse bytes into the panes", not leaked, f"{leaked}")

    # 5. New tab through the API.
    tabs_before = {t["tab_id"] for t in S.herdr_json("api", "snapshot")["result"]["snapshot"]["tabs"]}
    S.cmd({"cmd": "new_tab"})

    def tab_done():
        snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
        new = {t["tab_id"] for t in snap["tabs"]} - tabs_before
        if len(new) != 1:
            return None
        nt = next(iter(new))
        st = S.state()
        pane = next(p["pane_id"] for p in snap["panes"] if p["tab_id"] == nt)
        s = surf(st)
        if st["selected_tab"] == nt and pane in s and any("%" in l or "$" in l for l in s[pane]["visible_nonblank"]) \
                and s[pane]["cols"] > 10:
            return nt, pane, s[pane]
        return None
    got, dt = wait_for(tab_done, timeout=10)
    check("new tab (herdr API): tab exists in herdr, app selected it, its surface rendered a prompt", got is not None,
          f"{dt:.2f}s, {got[0]} {got[1]} {got[2]['cols']}x{got[2]['rows']}" if got else "")

    # Screenshot for the record.
    st = S.state()
    S.cmd({"cmd": "shot", "out": SHOT})
    time.sleep(1)
    say(f"screenshot: in-app capture -> {os.path.basename(SHOT)} exists={os.path.exists(SHOT)}")
    finish()


def finish():
    S.app("stop")
    time.sleep(0.5)
    left = S.sh("pgrep", "-f", f"{S.NAME}/bin/herdr terminal attach").split()
    check("attach clients exit with the app", not left, f"left={left}")
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
