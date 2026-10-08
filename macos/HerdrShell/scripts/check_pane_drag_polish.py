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
    state = D.wait(lambda s: D.zone_is(s, kind="centre", target=b))
    chip = D.pd(state).get("chip") or {}
    D.check("chip: sidebar state glyph precedes cap name", chip.get("glyph") in ("●", "◐", "✕", "✗", "■", "○", "·", "✓")
            and chip.get("text") == chip.get("glyph", "") + " " + chip.get("label", ""), json.dumps(chip))
    D.check("chip: stroke is the hairline token", chip.get("stroke") == 0.5
            and chip.get("stroke") == D.pd(state).get("hairline"), json.dumps(chip))
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
        D.motion("freeze", ms=ms)
        alphas.append(D.pd(S.state()).get("ghostAlpha"))
    D.check("ghost: never rises across settle 0–200ms", all(isinstance(x, (int, float)) for x in alphas)
            and alphas[0] > 0.999 and alphas[-1] < 0.001
            and all(y <= x + 1e-6 for x, y in zip(alphas, alphas[1:])), json.dumps(alphas))
    D.motion("run")
    D.wait(D.idle)
    D.close(tab)


def transport(ws):
    origin, dest, a, b, x, y, _ = P.hover_dest(ws, "polish-transport")
    state = S.state()
    fill = D.pd(state).get("sidebarZoneFill") or {}
    D.check("into-tab: hovered row reports accent zone fill", fill.get("row") == "tab:" + dest
            and fill.get("alpha") in (0.12, 0.16), json.dumps(fill))
    D.motion("freeze", ms=450)
    state = D.wait(lambda s: s.get("selected_tab") == dest and set(D.pd(s).get("boxes") or {}) == {x, y})
    D.hook("move", steps=1, **D.centre(D.pd(state)["boxes"][y]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=y))
    D.hook("fail-next-drop")
    D.hook("drop")
    state = D.wait(lambda s: D.idle(s) and s.get("selected_tab") == origin)
    D.check("transport: failed spring drop sends tab.focus(origin)", P.focus_calls(state) == [dest, origin]
            and state.get("selected_tab") == origin, json.dumps([P.focus_calls(state), state.get("selected_tab")]))
    D.check("transport: server focus restored too", P.server_focused(ws) == [origin], json.dumps(P.server_focused(ws)))
    D.motion("run")
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
    D.finish()


if __name__ == "__main__":
    main()
