#!/usr/bin/env python3
"""AGENTS rows say where each chat runs, in the Cua Space: "box" for a pane the move runner
adopted onto Alex's Rails box, else this machine's registry name ("Studio").

Run: HERDR_SHELL_SPACE=1 HERDR_SHELL_BIN=<herdr with TabInfo.runs_on> python3 scripts/check_runs_on_space.py.
check_agents_hide_home owns the row contract; this one covers the live path: the lab server's
scheduled poll reads its own rails-host/adopted.json and factory machines.json under the lab
HOME, tab.list carries runs_on, and the running app draws it. Writes checks/RUNS-ON-SPACE.txt
and 2x light/dark shots of the sidebar only; never launch on the host.
"""
import json
import os
import pathlib
import socket
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_runs_on_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-r"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/RUNS-ON-SPACE.txt")
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


def places(state):
    out = {}
    for row in state.get("spaces_rows", []):
        fields = row.split("|")
        if fields[1].startswith("agent:"):
            out[fields[1][len("agent:"):]] = next((f[3:] for f in fields if f.startswith("on:")), None)
    return out


def frame(text):
    return [float(v) for v in text.replace("{", "").replace("}", "").split(",")]


def crop_sidebar(png, state):
    from PIL import Image
    img = Image.open(png)
    scale = img.width / frame(state["window_frame"])[2]
    x, y, w, h = frame(state["theme"]["sidebar_frame"])
    img.crop((int(x * scale), int(y * scale), int((x + w) * scale), int((y + h) * scale))).save(png)


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
    factory, recruiter = [api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
                          for name in ("Factory", "Recruiter")]
    tabs = (cos, factory, recruiter)
    for tab in tabs:
        S.lab("herdr", "tab", "set-role", tab, "agent")
    pane = {p["tab_id"]: p["pane_id"] for p in api("pane", "list")["panes"]}
    home = pathlib.Path(dict(l.split("=", 1) for l in S.lab("env").splitlines())["HOME"])
    # The lab server's HOME holds what Studio's does: the factory registry naming this host
    # Studio, and the move runner's record that CoS's session moved onto the box.
    host = socket.gethostname().removesuffix(".local").lower()
    registry = home / ".agent-rails/factory-runtime/fleet/config/machines.json"
    registry.parent.mkdir(parents=True, exist_ok=True)
    registry.write_text(json.dumps([{"name": "studio", "aliases": ["Studio", host], "ssh": None}]))
    adopted = home / ".agent-rails/rails-host/adopted.json"
    adopted.parent.mkdir(parents=True, exist_ok=True)
    adopted.write_text(json.dumps({"panes": {pane[cos]: {"session_id": "as_lab", "host_id": "hst_box", "target": "box"}}}))
    listed = {}
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        listed = {t["tab_id"]: t.get("runs_on") for t in api("tab", "list")["tabs"]}
        if listed.get(cos) == "box":
            break
        time.sleep(1)
    check("tab.list: CoS runs on box, Factory and Recruiter on Studio",
          [listed.get(t) for t in tabs] == ["box", "Studio", "Studio"], json.dumps(listed))

    S.app("start")
    S.cmd({"cmd": "activate"})
    state = wait(lambda s: places(s).get(cos) == "box" and places(s).get(factory) == "Studio")
    got = places(state)
    check("the app's AGENTS rows draw box, Studio, Studio", [got.get(t) for t in tabs] == ["box", "Studio", "Studio"], json.dumps(got))
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        state = wait(lambda s: s.get("theme", {}).get("effective") == mode)
        check(mode + " appearance applied", state.get("theme", {}).get("effective") == mode)
        S.cmd({"cmd": "select", "tab": recruiter})
        wait(lambda s: s.get("selected_tab") == recruiter)
        time.sleep(0.5)
        state = S.state()
        png = str(ROOT / f"checks/RUNS-ON-SPACE-{mode}.png")
        S.cmd({"cmd": "shot", "out": png, "scale": 2})
        time.sleep(1.5)
        crop_sidebar(png, state)
        check(mode + ": sidebar shot written", pathlib.Path(png).stat().st_size > 0)
    S.cmd({"cmd": "appearance", "mode": "light"})


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
