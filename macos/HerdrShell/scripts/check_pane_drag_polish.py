#!/usr/bin/env python3
"""S11 real Mac Shell polish scenarios in Cua, with frozen time and a failed network edge.

Imports existing lab drivers without editing them. Geometry and calls come from the live
Shell/server; only the clock and one transport failure are injected at their boundaries.
"""
import json
import check_pane_drag_spring as P

D, S = P.D, P.S
S.OUT = str(D.ROOT / "checks/PANE-DRAG-POLISH.txt")


def settle(ws):
    tab, a, b, c, state = D.fresh(ws, "polish-settle")
    D.motion("freeze", ms=0)
    D.hook("begin", pane=a)
    D.hook("move", steps=1, **D.centre(D.pd(state)["boxes"][b]))
    state = D.wait(lambda s: D.zone_is(s, kind="centre", target=b) and (D.pd(s).get("chip") or {}).get("stroke", 0) > 0)
    chip = D.pd(state).get("chip") or {}
    D.check("chip: sidebar state glyph precedes cap name", chip.get("glyph") == {"working": "●", "needs": "■", "blocked": "■", "idle": "○", "asleep": "·", "done": "✓"}.get(chip.get("sourceState"))
            and chip.get("text") == chip.get("glyph", "") + " " + chip.get("label", ""), json.dumps(chip))
    D.check("chip: stroke is the hairline token", chip.get("stroke") == 0.5
, json.dumps(chip))
    D.hook("hold-replies", on=True)
    D.hook("drop")
    state = D.wait(lambda s: D.pd(s).get("replyHeld"))
    before = []
    for ms in range(0, 201, 10):
        D.motion("freeze", ms=ms)
        before.append(D.pd(S.state()).get("ghostAlpha"))
    D.check("ghost: full alpha from release until settle starts", before == [1] * 21, json.dumps(before))
    D.motion("freeze", ms=0)
    D.hook("send-reply")
    D.hook("hold-replies", on=False)
    D.wait(lambda s: D.pd(s).get("phase") == "settling")
    alphas = []
    for ms in range(0, 201, 10):
        D.motion("advance", ms=ms)
        alphas.append(D.pd(S.state()).get("ghostAlpha"))
    D.check("ghost: never rises across production pruning 0–200ms", all(isinstance(x, (int, float)) for x in alphas)
            and alphas[0] > 0.999 and alphas[-1] < 0.001
            and all(y <= x + 1e-6 for x, y in zip(alphas, alphas[1:])), json.dumps(alphas))
    D.motion("run")
    state = D.wait(D.idle)
    layers = D.pd(state).get("surfaceLayers") or []
    D.check("settle: surfaces return to static z-order without drag backing", bool(layers)
            and all(v.get("z") == 0 and not v.get("backed") for v in layers), json.dumps(layers))
    D.close(tab)


def transport(ws, timeout=False):
    origin, dest, a, b, x, y, _ = P.hover_dest(ws, "polish-transport")
    state = D.wait(lambda s: (D.pd(s).get("sidebarZoneFill") or {}).get("width", 0) > 0)
    fill = D.pd(state).get("sidebarZoneFill") or {}
    D.check("into-tab: hovered row reports accent zone fill", fill.get("row") == "tab:" + dest
            and any(abs(fill.get("alpha", 0) - a) < 1e-6 for a in (0.12, 0.16)) and fill.get("height", 0) > 0, json.dumps(fill))
    D.motion("freeze", ms=450)
    state = D.wait(lambda s: s.get("selected_tab") == dest and set(D.pd(s).get("boxes") or {}) == {x, y})
    D.hook("move", steps=1, **D.centre(D.pd(state)["boxes"][y]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=y))
    D.hook("lose-next-drop" if timeout else "fail-next-drop")
    D.hook("drop")
    state = D.wait(lambda s: D.idle(s) and s.get("selected_tab") == origin, timeout=10)
    D.check("lost connection after spring restores origin (win parity)", P.focus_calls(state) == [dest, origin]
            and state.get("selected_tab") == origin, json.dumps([P.focus_calls(state), state.get("selected_tab")]))
    D.check("transport: server focus restored too", P.server_focused(ws) == [origin], json.dumps(P.server_focused(ws)))
    D.motion("run")
    D.close(origin)
    D.close(dest)


def quiet_pending(ws):
    origin, dest, a, b, x, y, _ = P.hover_dest(ws, "polish-quiet")
    D.motion("freeze", ms=450)
    state = D.wait(lambda s: s.get("selected_tab") == dest and set(D.pd(s).get("boxes") or {}) == {x, y})
    D.hook("move", steps=1, **D.centre(D.pd(state)["boxes"][y]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=y))
    D.hook("hold-drops", on=True)
    D.hook("drop")
    D.wait(lambda s: D.pd(s).get("phase") == "dropping")
    D.hook("begin", pane=x)
    state = D.wait(lambda s: D.pd(s).get("phase") == "lifted")
    D.check("quiet: a new drag during a spring drop does not restore origin",
            state.get("selected_tab") == dest and P.focus_calls(state) == [], json.dumps(P.focus_calls(state)))
    D.hook("hold-drops", on=False)
    D.hook("cancel", via="esc")
    D.motion("run")
    D.wait(D.idle)
    D.close(origin)
    D.close(dest)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    made = D.api("workspace", "create", "--label", "polish", "--no-focus")
    ws = made["workspace"]["workspace_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})
    D.wait(lambda s: s.get("window_key") and "paneDrag" in s)
    settle(ws)
    transport(ws)
    transport(ws, timeout=True)
    quiet_pending(ws)
    D.finish()


if __name__ == "__main__":
    main()
