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
    # The sidebar's rule (SpacesTree.statusGlyph, Windows Sidebar.tsx Status): a square for a blocked
    # chat, a dot for every other state. A chip glyph set of its own (✓ ○ ·) fails here.
    rule = "■" if chip.get("sourceState") in ("needs", "blocked") else "●"
    D.check("chip: glyph follows the sidebar rule (■ blocked or needs, ● otherwise)", chip.get("glyph") == rule
            and chip.get("text") == rule + " " + chip.get("label", ""), json.dumps(chip))
    row = next((r.split("|") for r in state.get("spaces_rows", []) if r.split("|")[1] == "tab:" + tab), None)
    row_tone = (state.get("spaces_row_tones") or {}).get("tab:" + tab)
    D.check("chip: glyph and painted tone match the source tab's sidebar row as drawn", row is not None
            and chip.get("glyph") == row[4] and isinstance(row_tone, str) and chip.get("tone") == row_tone,
            json.dumps([chip.get("glyph"), chip.get("tone"), row and row[4], row_tone]))
    hairline = D.pd(state).get("hairline")
    D.check("chip: stroke is the hairline token the dump reports", isinstance(hairline, (int, float)) and hairline > 0
            and chip.get("stroke") == hairline, json.dumps([chip.get("stroke"), hairline]))
    # Use the live drawing frame, not a recomputed placement, against its window host bounds.
    width, height = D.pd(state)["chipBounds"]
    inset, offset = D.pd(state)["chipInset"], D.pd(state)["chipOffset"]
    D.hook("move", steps=1, x=width - 4, y=height - 4)
    state = D.wait(lambda s: (D.pd(s).get("chip") or {}).get("frame") != chip.get("frame"))
    frame = D.pd(state)["chip"]["frame"]
    x, y, w, h = frame
    D.check("chip: bottom-right drag keeps the whole frame inside window bounds",
            x >= inset and y >= inset and x + w <= width - inset and y + h <= height - inset,
            json.dumps([frame, [width, height], inset]))
    D.hook("move", steps=1, x=width / 2, y=height / 2)
    state = D.wait(lambda s: D.pd(s)["chip"]["frame"] != frame)
    middle = D.pd(state)["chip"]["frame"]
    D.check("chip: middle drag retains the existing pointer offset",
            abs(middle[0] - (width / 2 + offset)) < 0.01
            and abs(middle[1] - (height / 2 + offset)) < 0.01, json.dumps(middle))
    D.hook("move", steps=1, **D.centre(D.pd(state)["boxes"][b]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=b))
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
    # `begin` ends the pending drop and only then clears `sent`, so read the append-only `sendLog`:
    # every call from here on, the quiet end inside `begin` included.
    mark = max([c.get("seq", 0) for c in D.pd(D.S.state()).get("sendLog") or []], default=0)
    D.hook("begin", pane=x)
    state = D.wait(lambda s: D.pd(s).get("phase") == "lifted")
    D.check("quiet: a new drag during a spring drop keeps the Shell on dest", state.get("selected_tab") == dest,
            json.dumps(state.get("selected_tab")))
    D.hook("hold-drops", on=False)
    D.hook("cancel", via="esc")
    D.motion("run")
    state = D.wait(D.idle)
    since = [c for c in D.pd(state).get("sendLog") or [] if c.get("seq", 0) > mark]
    focus = [c.get("params", {}).get("tab_id") for c in since if c.get("method") == "tab.focus"]
    D.check("quiet: no tab.focus from the quiet end through the new drag's end", mark > 0 and focus == []
            and state.get("selected_tab") == dest, json.dumps([mark, since, state.get("selected_tab")]))
    D.check("quiet: the server keeps dest focused", P.server_focused(ws) == [dest], json.dumps(P.server_focused(ws)))
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
