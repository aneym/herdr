#!/usr/bin/env python3
"""Mac half of a Shell parity screenshot pair set, taken in the Cua Space.

Run: HERDR_SHELL_SPACE=1 python3 scripts/parity_shots.py OUT_DIR
Seeds a lab with an AGENTS section, two pins and a terminal pane, then shoots each
surface in light and dark as OUT_DIR/mac-<surface>-<mode>.png. The PC half comes from
windows/HerdrShell/scripts/pc.py shot. Screenshots, not assertions: a person or the
Opus design pass compares the pairs. Never launch on the host.

Surfaces: main, areas, detail, desk, factory. The factory view reads the host's
overlay, boxes, disk and routing files (pool state with key fields removed).
"""
import json
import os
import pathlib
import sys
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("parity_shots requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-parity"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
import scenario as S  # noqa: E402

OUT = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "parity-shots").resolve()
FIX = pathlib.Path(S.LAB) / "fixtures"
HOME = pathlib.Path.home()


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    print("wait timed out", flush=True)
    return state


def shoot(surface):
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        wait(lambda s: s.get("theme", {}).get("effective") == mode)
        time.sleep(0.5)
        path = OUT / f"mac-{surface}-{mode}.png"
        S.cmd({"cmd": "shot", "out": str(path)})
        print(f"{surface} {mode}: {path}", flush=True)


def write(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, indent=2))


def strip_keys(obj):
    if isinstance(obj, dict):
        return {k: strip_keys(v) for k, v in obj.items() if k not in ("key", "key_secret")}
    if isinstance(obj, list):
        return [strip_keys(v) for v in obj]
    return obj


def pane_of(tab):
    snap = api("api", "snapshot")["snapshot"]
    return next(p["pane_id"] for p in snap["panes"] if p["tab_id"] == tab)


def meta(pane, agent, status, **tokens):
    S.lab("herdr", "pane", "report-agent", pane, "--source", "spike", "--agent", agent, "--state", status)
    args = ["herdr", "pane", "report-metadata", pane, "--source", "spike"]
    for k, v in tokens.items():
        args += ["--token", f"{k}={v}"]
    S.lab(*args)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    FIX.mkdir(parents=True, exist_ok=True)
    made = api("workspace", "create", "--label", "rails", "--cwd", str(FIX), "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = made["workspace"]["workspace_id"]
    content = made["tab"]["tab_id"]
    content_pane = made["root_pane"]["pane_id"]
    S.lab("herdr", "tab", "rename", content, "Content")
    tabs = {name: api("tab", "create", "--workspace", ws, "--label", name, "--cwd", str(FIX), "--no-focus")["tab"]["tab_id"]
            for name in ("Frank", "orchestrator", "herdr ui", "factory infra", "nav retro")}
    for name in ("orchestrator", "herdr ui"):
        S.lab("herdr", "tab", "pin", tabs[name])
    for tab in (content, tabs["Frank"]):
        S.lab("herdr", "tab", "set-role", tab, "agent")
    panes = {name: pane_of(t) for name, t in tabs.items()}
    meta(panes["orchestrator"], "claude", "working", kind="orchestrator", inbox="2",
         inbox_items="Outreach drafts drop the role title@plugin|Sidebar header height jumps@VPD",
         routed="recruiter drafts bug@10:52|workspace embed header@10:48")
    meta(panes["herdr ui"], "claude", "working", kind="lane")
    meta(panes["nav retro"], "codex", "blocked", kind="workflow", host="PC", phase="impl 3/5")
    S.lab("herdr", "agent", "owner", "set", panes["nav retro"], panes["herdr ui"])

    # Areas: two areas, the rails space in one, a lane on the other.
    write(FIX / "lanes.json", {"version": 1, "lanes": [
        {"tab": tabs["herdr ui"], "name": "herdr ui", "label": "herdr ui", "kind": "lane",
         "goal_area": "shell polish", "section": "building"},
        {"tab": tabs["factory infra"], "name": "factory infra", "label": "factory infra", "kind": "lane",
         "goal_area": "factory", "section": "scoping"}]})
    write(FIX / "areas.json", {"version": 1, "areas": [
        {"id": "herdr", "name": "herdr", "color": "#5AA9FF"},
        {"id": "factory", "name": "factory", "color": "#7CC08A"}],
        "tabs": {}, "spaces": {ws: "herdr"}, "goal_area": {"shell polish": "herdr", "factory": "factory"}})
    os.environ["HERDR_LANES_PATH"] = str(FIX / "lanes.json")
    os.environ["HERDR_AREAS_PATH"] = str(FIX / "areas.json")

    S.app("start")
    S.cmd({"cmd": "activate"})
    wait(lambda s: s.get("window_key") is True, 10)
    S.cmd({"cmd": "select", "tab": content})
    wait(lambda s: s.get("selected_tab") == content)
    S.cmd({"cmd": "type", "text": "ls /"})
    S.cmd({"cmd": "key", "key": "return"})
    time.sleep(1)
    shoot("main")

    S.cmd({"cmd": "click", "target": "mode", "label": "areas"})
    st = wait(lambda s: s.get("shell", {}).get("mode") == "areas", 8)
    print("areas mode:", st.get("shell", {}).get("mode"), flush=True)
    time.sleep(0.6)
    shoot("areas")
    S.cmd({"cmd": "click", "target": "mode", "label": "spaces"})
    wait(lambda s: s.get("shell", {}).get("mode") == "spaces", 8)

    S.cmd({"cmd": "detail", "action": "toggle", "row": tabs["orchestrator"]})
    st = wait(lambda s: (s.get("detail") or {}).get("open") is True, 8)
    print("detail open:", (st.get("detail") or {}).get("open"), (st.get("detail") or {}).get("title"), flush=True)
    time.sleep(0.6)
    shoot("detail")
    S.cmd({"cmd": "detail", "action": "close"})
    wait(lambda s: (s.get("detail") or {}).get("open") is False, 8)

    notes = FIX / "desk-notes.md"
    notes.write_text("# Shell parity notes\n\nOpened on the desk from the Content pane.\n\n"
                     "- sidebar rows keep name, dot and icons\n- light and dark pairs per surface\n"
                     "- PC half comes from pc.py shot\n")
    plan = FIX / "desk-plan.md"
    plan.write_text("# Next slice\n\n1. Factory row spacing\n2. Desk tab strip height\n")
    for doc in (plan, notes):
        S.lab("herdr", "pane", "run", content_pane, f"herdr desk open {doc}")
        time.sleep(1.5)
    st = wait(lambda s: s.get("docs_visible") is True and len((s.get("desk") or {}).get("items", [])) >= 2, 20)
    print("desk:", st.get("docs_visible"), json.dumps((st.get("desk") or {}).get("items", []))[:300], flush=True)
    time.sleep(0.8)
    shoot("desk")
    S.cmd({"cmd": "docs", "open": False})

    # Factory: relaunch with the host's factory files so the view has real rows.
    pool = HOME / ".agent-rails/factory/state/pool.json"
    if pool.exists():
        write(FIX / "pool.json", strip_keys(json.loads(pool.read_text())))
        os.environ["FACTORY_POOLSTATE"] = str(FIX / "pool.json")
    for env, path in (("FACTORY_OVERLAY", ".agent-rails/herdr/overlay.json"),
                      ("FACTORY_BOXES", ".agent-rails/factory/boxes.json"),
                      ("FACTORY_DISK", ".agent-rails/fleet-disk-watch/state.json"),
                      ("FACTORY_ROUTING", ".agent-lb/managed/coding-agents/routing-table.json")):
        if (HOME / path).exists():
            os.environ[env] = str(HOME / path)
    S.app("stop")
    time.sleep(0.6)
    S.app("start")
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "select", "tab": content})
    wait(lambda s: s.get("selected_tab") == content)
    S.cmd({"cmd": "factory", "open": True})
    wait(lambda s: s.get("shell", {}).get("factory_open") is True, 10)
    time.sleep(1.5)
    S.cmd({"cmd": "factory", "open": True, "expand": "implement"})
    time.sleep(6)
    shoot("factory")


if __name__ == "__main__":
    try:
        main()
    finally:
        S.app("stop")
        S.lab("down")
