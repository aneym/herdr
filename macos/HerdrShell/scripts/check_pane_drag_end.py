#!/usr/bin/env python3
"""Every pane drop ends (pane drag fix S6b): regressions beside check_pane_drag.py, on its helpers.

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 macos/HerdrShell/scripts/check_pane_drag_end.py
Same lab server rules as check_pane_drag.py (HERDR_SHELL_BIN must answer pane.place; else BLOCKED, exit 2).

  1. Reduce Motion, a release on a sidebar tab row: the source leaves the tab, so the layout brings
     no crossfade; the drop still ends (phase idle, no motion) instead of sticking in settling.
  2. The same for a release on a space header (a new tab in that space).
  3. A drop the server refuses (a centre swap with a pane closed under the drag) plays the cancel:
     the phase passes through cancelling with a cancel motion, then ends idle with the layout as it was.
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


def refused_drop(ws):
    tab, a, b, c, state = D.fresh(ws, "refused")
    before = D.rects(D.layout(a))
    D.motion("freeze", ms=0)
    D.hook("begin", pane=c)
    D.hook("move", **D.centre(D.pd(state)["boxes"][b]))
    D.wait(lambda s: D.zone_is(s, kind="centre", target=b))
    S.lab("herdr", "pane", "close", b)
    D.wait(lambda s: b not in (D.pd(s).get("boxes") or {}))
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
    D.check("refused: nothing moved but the closed pane", after.get(a) is not None and set(after) == {a, c},
            json.dumps([before, after]))
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
    D.finish()


if __name__ == "__main__":
    main()
