#!/usr/bin/env python3
"""Settings > Agents in the Cua Space: show and hide agents from Settings, synced through herdr.

Run: HERDR_SHELL_SPACE=1 HERDR_SHELL_BIN=<herdr with `tab show`> python3 scripts/check_settings_space.py.
A lab server with three agent pins (CoS, Factory, League Coach). The running app opens Settings
(the ⌘, action), switches League Coach off: herdr's tab.list reads hidden=true, the Agents rows drop
it while its pin keeps slot 2, and the sidebar shot shows the row gone. `herdr tab show` from the CLI
switches it back on in the open Settings page with no click, and the switch brings it back again
after a CLI hide. Writes checks/SETTINGS-SPACE.txt and 2x shots: Settings light/dark (whole window)
and the sidebar with League Coach hidden and shown. Never launches on the host.
"""
import json
import os
import pathlib
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_settings_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-sa"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/SETTINGS-SPACE.txt")
lines, failures = [], []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=30):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.3)
    return state


def frame(text):
    return [float(v) for v in text.replace("{", "").replace("}", "").split(",")]


def crop_sidebar(png, state):
    from PIL import Image
    img = Image.open(png)
    scale = img.width / frame(state["window_frame"])[2]
    x, y, w, h = frame(state["theme"]["sidebar_frame"])
    img.crop((int(x * scale), int(y * scale), int((x + w) * scale), int((y + h) * scale))).save(png)


def shown(state):
    return {r["tab"]: r["shown"] for r in state.get("settings_rows", [])}


def agent_rows(state):
    """Agent rows the sidebar draws, in order (hidden ones sit in the shut Hidden fold)."""
    return [row.split("|")[1][len("agent:"):] for row in state.get("spaces_rows", []) if row.split("|")[1].startswith("agent:")]


def hidden_on_server():
    return {t["tab_id"]: t.get("hidden", False) for t in api("tab", "list")["tabs"] if t.get("role") == "agent"}


def pin_index():
    return {t["tab_id"]: t.get("pin_index") for t in api("tab", "list")["tabs"] if t.get("role") == "agent"}


def shot(name, state, sidebar=False):
    png = str(ROOT / f"checks/{name}.png")
    # Settings scrolls in an NSScrollView, which the in-process 2x layer render leaves out; the
    # window server capture at the Space's own scale draws what the window shows.
    S.cmd({"cmd": "shot", "out": png} if not sidebar else {"cmd": "shot", "out": png, "scale": 2})
    time.sleep(1.5)
    if sidebar:
        crop_sidebar(png, state)
    check(name + " shot written", pathlib.Path(png).stat().st_size > 0)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "agents", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = made["workspace"]["workspace_id"]
    cos = made["tab"]["tab_id"]
    S.lab("herdr", "tab", "rename", cos, "CoS")
    factory, coach = [api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
                      for name in ("Factory", "League Coach")]
    tabs = (cos, factory, coach)
    for tab in tabs:
        S.lab("herdr", "tab", "set-role", tab, "agent")
    slots = pin_index()
    check("three agent pins in order", [slots.get(t) for t in tabs] == [0, 1, 2], json.dumps(slots))

    S.app("start")
    S.cmd({"cmd": "activate"})
    state = wait(lambda s: agent_rows(s) == list(tabs))
    check("sidebar draws all three agents", agent_rows(state) == list(tabs), json.dumps(agent_rows(state)))

    S.cmd({"cmd": "settings", "open": True})
    state = wait(lambda s: s.get("settings_open") and len(s.get("settings_rows", [])) == 3)
    check("Settings opens with every agent switched on",
          state.get("settings_open") and [shown(state).get(t) for t in tabs] == [True, True, True], json.dumps(state.get("settings_rows")))
    check("Settings rows follow the pin order", [r["tab"] for r in state.get("settings_rows", [])] == list(tabs))
    check("Settings covers the whole window", state.get("settings_frame", [])[2:] == state.get("window_content"),
          f'{state.get("settings_frame")} vs {state.get("window_content")}')
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        state = wait(lambda s: s.get("theme", {}).get("effective") == mode)
        time.sleep(0.5)
        shot(f"SETTINGS-AGENTS-{mode}", state)

    # The switch: League Coach off.
    S.cmd({"cmd": "appearance", "mode": "light"})
    S.cmd({"cmd": "settings_toggle", "tab": coach})
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline and not hidden_on_server().get(coach):
        time.sleep(0.3)
    check("switch off: herdr tab.list reads hidden=true", hidden_on_server().get(coach) is True, json.dumps(hidden_on_server()))
    check("hidden pin keeps its slot", pin_index().get(coach) == 2, json.dumps(pin_index()))
    state = wait(lambda s: shown(s).get(coach) is False and coach not in agent_rows(s))
    check("Settings shows League Coach off", shown(state).get(coach) is False, json.dumps(state.get("settings_rows")))
    check("Agents rows drop League Coach", agent_rows(state) == [cos, factory], json.dumps(agent_rows(state)))
    shot("SETTINGS-AGENTS-off-light", state)
    S.cmd({"cmd": "settings", "open": False})
    state = wait(lambda s: not s.get("settings_open"))
    shot("SETTINGS-SIDEBAR-hidden-light", state, sidebar=True)

    # Another client (the CLI) shows it again; the open page follows with no click.
    S.cmd({"cmd": "settings", "open": True})
    S.lab("herdr", "tab", "show", coach)
    state = wait(lambda s: shown(s).get(coach) is True and agent_rows(s) == list(tabs))
    check("`herdr tab show` turns the switch back on", shown(state).get(coach) is True, json.dumps(state.get("settings_rows")))
    check("League Coach is back in its old slot", agent_rows(state) == list(tabs), json.dumps(agent_rows(state)))

    # And a CLI hide, brought back by the switch.
    S.lab("herdr", "tab", "hide", coach)
    state = wait(lambda s: shown(s).get(coach) is False)
    check("`herdr tab hide` turns the switch off", shown(state).get(coach) is False)
    S.cmd({"cmd": "settings_toggle", "tab": coach})
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline and hidden_on_server().get(coach):
        time.sleep(0.3)
    check("switch on: herdr tab.list drops hidden", hidden_on_server().get(coach) is False, json.dumps(hidden_on_server()))
    state = wait(lambda s: agent_rows(s) == list(tabs))
    check("switch on: League Coach back in its slot", agent_rows(state) == list(tabs), json.dumps(agent_rows(state)))
    S.cmd({"cmd": "settings", "open": False})
    state = wait(lambda s: not s.get("settings_open"))
    check("Done closes Settings", not state.get("settings_open"))
    shot("SETTINGS-SIDEBAR-shown-light", state, sidebar=True)


if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        check("scenario completed", False, str(exc))
    finally:
        try:
            S.app("stop")
            S.lab("down")
        finally:
            pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
            pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))
