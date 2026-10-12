#!/usr/bin/env python3
"""Every Spaces sidebar row and control answers a real click (G2, agent-rails #2732).

Run: HERDR_SHELL_SPACE=1 python3 scripts/check_click_all.py [--shots DIR]   (writes checks/CLICK-ALL.txt)

Alex, 2026-09-28 21:36Z: "nothing like the mock, i cant click on shit". The dev build
(.build/release/HerdrShell) runs in the Cua Space against a lab seeded like the factory
mock (~/.claude/pretty-docs/factory-devenv-mock-2026-09-28.html): an agent-rails space with
an orchestrator, lanes with PC workflows, a background group, agents and a pin, a plain
homebase and a hosts footer. Each click is a CGEvent posted by cua-driver in the guest at
the target's drawn frame (the state dump's row_frames), never a hook call, and each must
change what the app shows: a tab row selects its tab, a chevron or header folds, a pin
toggles the pin, + makes a tab. --shots also saves light and dark frames for the mock
side-by-side.
"""
import json
import os
import pathlib
import sys
import time

from PIL import Image

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_click_all requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-click"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.setdefault("HERDR_SPACE_OWNER", "check-click-all")
ROOT = pathlib.Path(__file__).resolve().parents[1]
LAB = pathlib.Path.home() / ".cache/herdr-build/shellspike-click"
LAB.mkdir(parents=True, exist_ok=True)
os.environ["FACTORY_OVERLAY"] = str(LAB / "overlay.json")
sys.path.insert(0, str(ROOT / "scripts"))
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/CLICK-ALL.txt")
SHOTS = pathlib.Path(sys.argv[sys.argv.index("--shots") + 1]).resolve() if "--shots" in sys.argv else None
lines, failures = [], []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=6):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    return state


def rows(state):
    """Spaces rows by id: kind, chevron, title, trailing and tab, as SpacesRow.dump writes them."""
    out = {}
    for line in state.get("spaces_rows", []):
        f = line.split("|")
        out[f[1]] = {"kind": f[0], "depth": int(f[2]), "chevron": f[3], "title": f[6], "trailing": f[7], "tab": f[10]}
    return out


def seed():
    old = api("workspace", "list")["workspaces"]
    ws, first = {}, {}
    for name in ("agent-rails", "homebase"):
        made = api("workspace", "create", "--label", name, "--no-focus")
        ws[name], first[name] = made["workspace"]["workspace_id"], made["tab"]["tab_id"]
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    t = {"rails orchestrator": first["agent-rails"], "aside hls playback": first["homebase"]}
    S.lab("herdr", "tab", "rename", t["rails orchestrator"], "rails orchestrator")
    S.lab("herdr", "tab", "rename", t["aside hls playback"], "aside hls playback")
    for name in ("CoS", "Recruiter", "recruiter", "wf recruiter-2320", "wf recruiter-2315", "workspace embed",
                 "wf embed wave-a", "agent blocks", "factory infra", "conversation evals", "step back", "recruiting email"):
        t[name] = api("tab", "create", "--workspace", ws["agent-rails"], "--label", name, "--no-focus")["tab"]["tab_id"]
    t["dev server"] = api("tab", "create", "--workspace", ws["homebase"], "--label", "dev server", "--no-focus")["tab"]["tab_id"]
    for name in ("CoS", "Recruiter"):
        S.lab("herdr", "tab", "set-role", t[name], "agent")
    S.lab("herdr", "tab", "pin", t["factory infra"])
    now = time.time()
    wf = lambda parent, **k: {"kind": "workflow", "parent": t[parent], "badge": "PC", **k}
    overlay = {"version": 1, "generated_at": now, "tabs": {
        t["rails orchestrator"]: {"kind": "orchestrator", "name": "rails orchestrator", "summary": "inbox 3"},
        t["recruiter"]: {"kind": "lane"}, t["workspace embed"]: {"kind": "lane"},
        t["wf recruiter-2320"]: wf("recruiter", phase="impl", started=now - 1200),
        t["wf recruiter-2315"]: wf("recruiter", phase="review", started=now - 3000),
        t["wf embed wave-a"]: wf("workspace embed", phase="fix", started=now - 600),
        t["agent blocks"]: {"kind": "lane"}, t["factory infra"]: {"kind": "lane"},
        t["conversation evals"]: {"kind": "lane", "idle_reason": "idle"},
        t["step back"]: {"kind": "advisor"}, t["recruiting email"]: {"kind": "lane", "done": True}},
        "spaces": {ws["agent-rails"]: {"summary": "factory"}, ws["homebase"]: {"summary": "plain"}},
        "hosts": [{"name": "Studio", "summary": "load 9/16"}, {"name": "PC", "summary": "3/28 live"}, {"name": "forge", "summary": "0/4"}]}
    pathlib.Path(os.environ["FACTORY_OVERLAY"]).write_text(json.dumps(overlay))
    return ws, t


def app_window():
    pid = int(json.loads(S.space("status"))["app"]["pid"])
    out = S.space("driver", "list_windows", json.dumps({"pid": pid}))
    wins = [json.loads(l) for l in out.splitlines() if l.startswith("{")][0]["structured"]["windows"]
    win = max(wins, key=lambda w: w["bounds"]["width"] * w["bounds"]["height"])
    return pid, win["window_id"], win["bounds"]


def click_at(pid, window, state, x, y):
    """One left click at window point (x, y) from the top left, as a pointer CGEvent in the guest."""
    scale = state["backing_scale"]
    S.space("driver", "get_window_state", json.dumps({"pid": pid, "window_id": window, "include_screenshot": True}),
            "click", json.dumps({"pid": pid, "window_id": window, "x": round(x * scale), "y": round(y * scale),
                                 "delivery_mode": "foreground"}))


def hover_at(bounds, x, y):
    """Pointer moves to window point (x, y), as mouse-moved CGEvents posted in the guest.
    cua-driver's move_cursor warps the pointer, which fires no tracking area, so hover never shows."""
    x, y = bounds["x"] + x, bounds["y"] + y
    js = "ObjC.import('CoreGraphics');" + "".join(
        f"$.CGEventPost($.kCGHIDEventTap, $.CGEventCreateMouseEvent(null, $.kCGEventMouseMoved, $.CGPointMake({x + d}, {y}), 0));delay(0.05);"
        for d in (-4, -2, 0))
    S.space("exec", "osascript -l JavaScript -e " + json.dumps(js))


def frame(state, key):
    return state.get("row_frames", {}).get(key)


def shoot(name):
    if not SHOTS:
        return
    SHOTS.mkdir(parents=True, exist_ok=True)
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        time.sleep(0.8)
        S.cmd({"cmd": "shot", "out": str(SHOTS / f"{name}-{mode}.png")})
    S.cmd({"cmd": "appearance", "mode": "system"})


def pin_glyphs(pid, window):
    """How many pin glyphs the window draws, from cua-driver's accessibility tree."""
    out = S.space("driver", "get_window_state", json.dumps({"pid": pid, "window_id": window, "include_screenshot": False}))
    trees = [json.loads(line).get("text") or "" for line in out.splitlines() if line.startswith("{")]
    return sum(tree.count('AXStaticText = "\u26b2"') for tree in trees)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    for domain in ("herdr.shell.shellspike-click", "herdr.shell.dev.shellspike-click"):
        S.space("exec", "defaults delete " + domain + " 2>/dev/null; true")
    ws, t = seed()
    S.app("start")
    S.cmd({"cmd": "activate"})
    state = wait(lambda s: s.get("window_key") is True and "tab:" + t["recruiter"] in rows(s)
                 and frame(s, "tab:" + t["recruiter"]), 30)
    check("dev app is key in the Space with the seeded spaces drawn", "tab:" + t["recruiter"] in rows(state))
    pid, window, bounds = app_window()

    def at(key, dx=None):
        """Click the centre of a drawn target (or dx points in from its left edge)."""
        state = S.state()
        f = frame(state, key)
        if f:
            click_at(pid, window, state, f[0] + (f[2] / 2 if dx is None else min(dx, f[2] / 2)), f[1] + f[3] / 2)
        return f

    def hover(key):
        """Rest the pointer on a drawn row, 40 points in from its left edge."""
        f = frame(S.state(), key)
        if f:
            hover_at(bounds, f[0] + 40, f[1] + f[3] / 2)

    def pane_click():
        """A click in the terminal pane: moves the pointer off the sidebar."""
        state = S.state()
        cap = next(iter(state.get("pane_caps", [])), None)
        if cap:
            f = cap["window_frame"]
            hover_at(bounds, f[0] + f[2] / 2, f[1] + f[3] + 120)
            click_at(pid, window, state, f[0] + f[2] / 2, f[1] + f[3] + 120)

    # At rest a row shows name, dot and icons only: no pin or + until the pointer is on it.
    S.cmd({"cmd": "select", "tab": t["dev server"]})
    pane_click()
    time.sleep(0.6)
    shoot("sidebar")
    glyphs = pin_glyphs(pid, window)
    check("no pin glyph on any row at rest", glyphs == 0, f"pin glyphs={glyphs}")

    # A tab row selects its tab. Start each from another tab so the change is visible.
    away = t["aside hls playback"]
    tab_rows = [rid for rid, r in rows(S.state()).items() if r["kind"] == "tab" and r["tab"]]
    for rid in tab_rows:
        target = rows(S.state())[rid]["tab"]
        S.cmd({"cmd": "select", "tab": away if target != away else t["dev server"]})
        wait(lambda s: s.get("selected_tab") != target, 3)
        f = at(rid, dx=40)
        state = wait(lambda s: s.get("selected_tab") == target)
        check(f"row {rid} ({rows(state).get(rid, {}).get('title', '?')}) selects its tab",
              f is not None and state.get("selected_tab") == target, f"frame={f} selected={state.get('selected_tab')}")

    # A space row selects its space's tab.
    for rid in [rid for rid, r in rows(S.state()).items() if r["kind"] == "space"]:
        target = rows(S.state())[rid]["tab"]
        S.cmd({"cmd": "select", "tab": t["CoS"]})
        wait(lambda s: s.get("selected_tab") == t["CoS"], 3)
        f = at(rid, dx=40)
        state = wait(lambda s: s.get("selected_tab") == target)
        check(f"space row {rid} selects its first tab", state.get("selected_tab") == target, f"frame={f}")

    # Chevrons: a lane with workflows opens and closes; the background group opens.
    for rid in ("tab:" + t["recruiter"], "tab:" + t["workspace embed"]):
        for _ in range(2):
            before = rows(S.state())[rid]["chevron"]
            f = at("chevron:" + rid)
            state = wait(lambda s: rows(s)[rid]["chevron"] != before)
            check(f"lane chevron {rid} {before} -> {rows(state)[rid]['chevron']}", rows(state)[rid]["chevron"] != before, f"frame={f}")
    group = "group:" + ws["agent-rails"] + ":background"
    before = rows(S.state())[group]["chevron"]
    f = at(group, dx=40)
    state = wait(lambda s: rows(s)[group]["chevron"] != before)
    check("background group header opens", rows(state)[group]["chevron"] != before, f"frame={f}")

    # Pins: the pointer on a lane row shows its pin, and the pin pins the tab.
    lane = "tab:" + t["agent blocks"]
    pins_before = sum(1 for r in rows(S.state()) if r.startswith("pinned:"))
    hover(lane)
    state = wait(lambda s: frame(s, "pin:" + lane) is not None)
    f = at("pin:" + lane)
    state = wait(lambda s: sum(1 for r in rows(s) if r.startswith("pinned:")) != pins_before)
    check("lane row pin (on hover) pins the tab", sum(1 for r in rows(state) if r.startswith("pinned:")) == pins_before + 1, f"frame={f}")

    # A space row on hover: + makes a tab in that space, the pin pins the space.
    space = "space:" + ws["homebase"]
    tabs_before = len(api("tab", "list")["tabs"])
    hover(space)
    wait(lambda s: frame(s, "plus:" + space) is not None)
    f = at("plus:" + space)
    deadline = time.monotonic() + 6
    while len(api("tab", "list")["tabs"]) == tabs_before and time.monotonic() < deadline:
        time.sleep(0.3)
    check("space row + (on hover) makes a tab", len(api("tab", "list")["tabs"]) == tabs_before + 1, f"frame={f}")
    hover(space)
    wait(lambda s: frame(s, "pin:" + space) is not None)
    f = at("pin:" + space)
    state = wait(lambda s: ws["homebase"] in (s.get("spaces_chrome") or {}).get("pinnedSpaces", []))
    check("space row pin (on hover) pins the space", ws["homebase"] in (state.get("spaces_chrome") or {}).get("pinnedSpaces", []), f"frame={f}")
    pane_click()
    time.sleep(0.6)
    glyphs = pin_glyphs(pid, window)
    check("a pinned space keeps its pin at rest, nothing else", glyphs == 1, f"pin glyphs={glyphs}")

    # The space chevron folds the space under "hidden"; the hidden header opens it again.
    f = at("chevron:" + space)
    state = wait(lambda s: "hidden" in rows(s))
    check("space chevron folds the space under 'hidden'", "hidden" in rows(state) and space not in rows(state), f"frame={f}")
    f = at("hidden", dx=40)
    state = wait(lambda s: rows(s).get("hidden", {}).get("chevron") == "open")
    check("hidden header opens", rows(state).get("hidden", {}).get("chevron") == "open", f"frame={f}")

    # Collapse-all folds every space; a second click opens them.
    f = at("collapse-spaces")
    state = wait(lambda s: not any(r["kind"] == "space" and r["depth"] == 0 for r in rows(s).values()))
    check("collapse-all button folds every space", not any(r["kind"] == "space" and r["depth"] == 0 for r in rows(state).values()), f"frame={f}")
    f = at("collapse-spaces")
    state = wait(lambda s: any(r["kind"] == "space" and r["depth"] == 0 for r in rows(s).values()))
    check("collapse-all button opens them again", any(r["kind"] == "space" and r["depth"] == 0 for r in rows(state).values()), f"frame={f}")

    # Mode buttons.
    for mode in ("areas", "spaces"):
        f = at("mode:" + mode)
        state = wait(lambda s: (s.get("shell") or {}).get("mode") == mode)
        check(f"mode button {mode} switches the sidebar", (state.get("shell") or {}).get("mode") == mode, f"frame={f}")

    # The PINNED + makes a new pinned tab.
    tabs_before = len(api("tab", "list")["tabs"])
    f = at("pinned+")
    deadline = time.monotonic() + 6
    while len(api("tab", "list")["tabs"]) == tabs_before and time.monotonic() < deadline:
        time.sleep(0.3)
    check("PINNED + makes a tab", len(api("tab", "list")["tabs"]) == tabs_before + 1, f"frame={f}")

    # Pane header: the pin toggles the tab's pin; the ... menu opens.
    state = S.state()
    cap = next((c for c in state.get("pane_caps", []) if c.get("pinned") is not None), None)
    if cap:
        f = cap["window_frame"]
        before = cap["pinned"]
        # PaneCapBar: trailing 10, pin and menu 24 wide, 8 apart.
        click_at(pid, window, state, f[0] + f[2] - 10 - 24 - 8 - 12, f[1] + f[3] / 2)
        state = wait(lambda s: any(c["id"] == cap["id"] and c.get("pinned") != before for c in s.get("pane_caps", [])))
        check("pane header pin toggles the tab's pin",
              any(c["id"] == cap["id"] and c.get("pinned") != before for c in state.get("pane_caps", [])))
        # The menu is a popup window cua-driver does not list, so compare the Space desktop under
        # the button before and after the click: the open menu draws there.
        def under_menu():
            shot = LAB / "menu.png"
            S.space("shot", str(shot))
            image = Image.open(shot).convert("RGB")
            k = S.state()["backing_scale"]
            box = (bounds["x"] + f[0] + f[2] - 140, bounds["y"] + f[1] + f[3] + 6, bounds["x"] + f[0] + f[2] - 10, bounds["y"] + f[1] + f[3] + 40)
            return image.crop(tuple(round(v * k) for v in box)).tobytes()
        before_menu = under_menu()
        click_at(pid, window, state, f[0] + f[2] - 10 - 12, f[1] + f[3] / 2)
        time.sleep(0.8)
        check("pane header ... opens its menu", under_menu() != before_menu)
        S.space("driver", "press_key", json.dumps({"pid": pid, "window_id": window, "key": "escape"}))
    else:
        check("pane header has a pin", False, "no cap with a pin")
    shoot("after-clicks")


if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        import traceback
        check("scenario completed", False, repr(exc)[:400] + " " + traceback.format_exc()[-600:].replace("\n", " | "))
    finally:
        try:
            S.app("stop")
            S.lab("down")
        finally:
            lines.append(f"{sum(l.startswith('[PASS]') for l in lines)} passed; {len(failures)} failed")
            pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
            pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))
