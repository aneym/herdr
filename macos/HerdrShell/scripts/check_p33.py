#!/usr/bin/env python3
"""P33: sectioned Spaces, persistent folding, selection and empty-doc lifecycle.

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 scripts/check_p33.py.
The pure tree contract belongs to check_p33_parity; this checks real shell state.
"""
import json
import os
import pathlib
import subprocess
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("P33 requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-p33"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
LAB = pathlib.Path.home() / ".cache/herdr-build/shellspike-p33"
LAB.mkdir(parents=True, exist_ok=True)
os.environ["FACTORY_OVERLAY"] = str(LAB / "overlay.json")
import scenario as S

S.OUT = str(ROOT / "checks/P33.txt")
lines = []
failures = []


def check(name, condition):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}"
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
        time.sleep(0.2)
    return state


def click(row, part="body"):
    S.cmd({"cmd": "spaces_click", "row": row, "part": part})


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    # The app runs in the Space guest: clear its saved mode and folds there, or a previous run's fold leaks in.
    for domain in ("herdr.shell.shellspike-p33", "herdr.shell.dev.shellspike-p33"):
        subprocess.run(["defaults", "delete", domain], capture_output=True)
        S.space("exec", "defaults delete " + domain + " 2>/dev/null; true")
    old = api("workspace", "list")["workspaces"]
    workspaces = [api("workspace", "create", "--label", name, "--no-focus") for name in ("factory", "poker")]
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = workspaces[0]["workspace"]["workspace_id"]
    orch = workspaces[0]["tab"]["tab_id"]
    other = workspaces[1]["tab"]["tab_id"]
    tabs = {}
    for name in ("scope", "implementation", "parked", "workflow"):
        tabs[name] = api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
    overlay = {"tabs": {
        orch: {"kind": "orchestrator", "name": "orchestrator", "summary": "inbox 1", "attention": "act"},
        tabs["scope"]: {"kind": "lane", "section": "scoping", "goal": "rails", "name": "content writing agent",
                        "scope_url": "https://studio.tailf266ac.ts.net:8797/s/content", "attention": "act", "busy": True,
                        "runs": [{"id": "r1", "name": "content-room-mock", "phase": "opus-seat", "started": time.time() - 33060}]},
        tabs["implementation"]: {"kind": "lane", "section": "implementing", "goal": "recruiter"},
        tabs["parked"]: {"kind": "lane", "mode": "parked"},
        tabs["workflow"]: {"kind": "workflow", "parent": tabs["implementation"]},
        other: {"kind": "lane", "name": "outreach", "idle_reason": "no work"}}, "spaces": {}, "hosts": [{"name": "Studio", "summary": "load 1/16"}],
        "usage": [{"name": "claude", "summary": "2/8 · 73%"}, {"name": "codex", "summary": "5/5 · 100%"}]}
    pathlib.Path(os.environ["FACTORY_OVERLAY"]).write_text(json.dumps(overlay))
    # Desk file coverage lives in check_desk_space.py; the scope lane already has docs.
    S.app("start")
    state = wait(lambda s: len(s.get("spaces_rows", [])) >= 10)
    rows = state.get("spaces_rows", [])
    check("default mode is spaces", state.get("shell", {}).get("mode") == "spaces")
    check("agents then goal All", len(rows) >= 2 and "|agents|" in rows[0] and "|goal All|" in rows[1])
    sections = [row.split("|")[6] for row in rows if row.startswith("section|")]
    check("sections follow Ghostty order", sections[:3] == ["ORCHESTRATOR", "SCOPING", "IMPLEMENTING"])
    check("parked is folded with count", any("|closed|" in row and "|parked 1|" in row for row in rows))
    # The first hook command can land before the sidebar has drawn; resend once.
    for _ in range(2):
        click("tab:" + tabs["implementation"])
        state = wait(lambda s: s.get("selected_tab") == tabs["implementation"], timeout=10)
        if state.get("selected_tab") == tabs["implementation"]:
            break
        print("click missed; rows:\n" + "\n".join(state.get("spaces_rows", [])), flush=True)
    check("row selects tab without detail", state.get("selected_tab") == tabs["implementation"] and state.get("detail_open") is False)
    key = "section:" + ws + ":IMPLEMENTING"
    click(key)
    state = wait(lambda s: any(row.startswith("section|" + key + "|") and "|closed|" in row for row in s.get("spaces_rows", [])))
    check("IMPLEMENTING folds", any(row.startswith("section|" + key + "|") and "|closed|" in row for row in state.get("spaces_rows", [])))
    S.app("stop")
    S.app("start")
    state = wait(lambda s: ws + ":IMPLEMENTING" in s.get("spaces_chrome", {}).get("collapsedSections", []))
    check("fold survives relaunch", ws + ":IMPLEMENTING" in state.get("spaces_chrome", {}).get("collapsedSections", []))
    # Docs are shown per tab and only on request: open them on the tab that has them.
    S.cmd({"cmd": "select", "tab": tabs["scope"]})
    wait(lambda s: s.get("selected_tab") == tabs["scope"])
    S.cmd({"cmd": "docs", "open": True})
    state = wait(lambda s: s.get("docs_visible") is True)
    check("tab with docs shows column", state.get("docs_visible") is True)
    S.cmd({"cmd": "select", "tab": other})
    wait(lambda s: s.get("selected_tab") == other)
    S.cmd({"cmd": "docs", "open": True})
    state = wait(lambda s: s.get("selected_tab") == other and s.get("docs_visible") is False)
    check("tab without docs takes no column", state.get("docs_visible") is False)
    S.cmd({"cmd": "goal", "value": "rails"})
    state = wait(lambda s: any("|goal rails|" in row for row in s.get("spaces_rows", [])))
    rows = state.get("spaces_rows", [])
    check("goal retains orchestrator and matching lane", any("tab|tab:" + orch + "|" in row for row in rows) and any("tab|tab:" + tabs["scope"] + "|" in row for row in rows))
    check("goal removes nonmatching lane", not any("tab|tab:" + tabs["implementation"] + "|" in row for row in rows))
    S.cmd({"cmd": "goal", "value": None})
    for appearance in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": appearance})
        time.sleep(0.6)
        S.cmd({"cmd": "shot", "out": str(ROOT / f"checks/P33-{appearance}.png")})
    S.app("stop")
    S.lab("down")
    pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
