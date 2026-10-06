#!/usr/bin/env python3
"""Live AGENTS roles, numbering and section-confined mouse drags in the Cua Space.

Run: HERDR_SHELL_SPACE=1 python3 scripts/check_agents_space.py.
Unlike the static section check, this covers server polling and real mouse dispatch.
Writes checks/AGENTS-SPACE.txt and light/dark shots; never launch on the host.
"""
import json
import os
import pathlib
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_agents_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-ag"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/AGENTS-SPACE.txt")
lines, failures = [], []
ROW = 24


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


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
    return state


def row_ids(state):
    return [row.split("|")[1] for row in state.get("spaces_rows", [])]


def section_order(state, prefix):
    return [row[len(prefix):] for row in row_ids(state) if row.startswith(prefix)]


def server_tabs(ws):
    return api("tab", "list", "--workspace", ws)["tabs"]


def server_order(ws):
    return [t["tab_id"] for t in sorted((t for t in server_tabs(ws) if t.get("pin_index") is not None),
                                        key=lambda t: t["pin_index"])]


def settled(ws, agents, pins):
    return wait(lambda s: section_order(s, "agent:") == agents
                and section_order(s, "pinned:") == pins
                and s.get("numbered_tabs", [])[:len(agents + pins)] == agents + pins
                and server_order(ws) == agents + pins)


def drag(tab, dy):
    S.cmd({"cmd": "drag_pin", "row": "agent:" + tab, "dy": dy, "steps": 8, "interval": 0.04})
    return wait(lambda s: s.get("drag_running") is False and s.get("pin_drag", {}).get("dragged") is None, 10)


def assert_order(name, state, ws, agents, pins):
    check(name + " rows", section_order(state, "agent:") == agents and section_order(state, "pinned:") == pins)
    check(name + " numbered tabs", state.get("numbered_tabs", [])[:len(agents + pins)] == agents + pins)
    check(name + " server", server_order(ws) == agents + pins)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "rails", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = made["workspace"]["workspace_id"]
    content = made["tab"]["tab_id"]
    S.lab("herdr", "tab", "rename", content, "Content")
    frank, lane_a, lane_b = [api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
                              for name in ("Frank", "lane-a", "lane-b")]
    pins = [lane_a, lane_b]
    for tab in pins:
        S.lab("herdr", "tab", "pin", tab)
    for tab in (content, frank):
        S.lab("herdr", "tab", "set-role", tab, "agent")
    S.app("start")
    S.cmd({"cmd": "activate"})
    state = wait(lambda s: s.get("window_key") is True, 10)
    check("app is key for mouse dispatch", state.get("window_key") is True)
    state = settled(ws, [content, frank], pins)
    ids = row_ids(state)
    expected = ["agentpins", "agent:" + content, "agent:" + frank, "pinned", "pinned:" + lane_a, "pinned:" + lane_b]
    check("AGENTS section and rows precede PINNED section and rows",
          [row for row in ids if row in expected] == expected)
    check("no agents title row above the AGENTS section", "agents" not in ids and ids[:1] == ["agentpins"])
    check("Content and Frank are absent as plain space tab rows", all("tab:" + t not in ids for t in (content, frank)))
    assert_order("initial", state, ws, [content, frank], pins)
    tabs = {t["tab_id"]: t for t in server_tabs(ws)}
    check("server agent roles have pin_index 0 and 1",
          all(tabs[t].get("role") == "agent" and tabs[t].get("pin_index") == i for i, t in enumerate((content, frank))))
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        state = wait(lambda s: s.get("theme", {}).get("effective") == mode)
        check(mode + " appearance applied", state.get("theme", {}).get("effective") == mode)
        S.cmd({"cmd": "shot", "out": str(ROOT / f"checks/AGENTS-SPACE-{mode}.png")})
    S.cmd({"cmd": "appearance", "mode": "light"})
    wait(lambda s: s.get("theme", {}).get("effective") == "light")

    drag(frank, -ROW)
    state = settled(ws, [frank, content], pins)
    assert_order("Frank dragged up", state, ws, [frank, content], pins)
    before_rows, before_server = row_ids(state), server_tabs(ws)
    drag(content, 6 * ROW)
    time.sleep(0.5)
    state = S.state()
    check("drag into PINNED leaves every row and server tab unchanged",
          row_ids(state) == before_rows and server_tabs(ws) == before_server)
    assert_order("cross-section drop", state, ws, [frank, content], pins)

    S.cmd({"cmd": "set_role", "tab": frank, "role": None})
    state = settled(ws, [content], [frank] + pins)
    assert_order("Frank role cleared", state, ws, [content], [frank] + pins)
    check("server omits Frank role", "role" not in next(t for t in server_tabs(ws) if t["tab_id"] == frank))
    S.cmd({"cmd": "set_role", "tab": frank, "role": "agent"})
    state = settled(ws, [content, frank], pins)
    assert_order("Frank restored at AGENTS end", state, ws, [content, frank], pins)
    check("server restores Frank agent role", next(t for t in server_tabs(ws) if t["tab_id"] == frank).get("role") == "agent")
    S.cmd({"cmd": "select", "tab": lane_b})
    wait(lambda s: s.get("selected_tab") == lane_b)
    drag(content, 0)
    state = wait(lambda s: s.get("selected_tab") == content, 5)
    check("a press in place on Content selects its tab", state.get("selected_tab") == content)


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
