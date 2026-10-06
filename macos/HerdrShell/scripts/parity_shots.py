#!/usr/bin/env python3
"""Mac half of a Shell parity screenshot pair set, taken in the Cua Space.

Run: HERDR_SHELL_SPACE=1 python3 scripts/parity_shots.py OUT_DIR
Seeds a lab with an AGENTS section, two pins and a terminal pane, then shoots each
surface in light and dark as OUT_DIR/mac-<surface>-<mode>.png. The PC half comes from
windows/HerdrShell/scripts/pc.py shot. Screenshots, not assertions: a person or the
Opus design pass compares the pairs. Never launch on the host.
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


def shoot(surface):
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        wait(lambda s: s.get("theme", {}).get("effective") == mode)
        time.sleep(0.4)
        path = OUT / f"mac-{surface}-{mode}.png"
        S.cmd({"cmd": "shot", "out": str(path)})
        print(f"{surface} {mode}: {path}", flush=True)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
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
    tabs = {name: api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
            for name in ("Frank", "orchestrator", "herdr ui", "factory infra", "nav retro")}
    for name in ("orchestrator", "herdr ui"):
        S.lab("herdr", "tab", "pin", tabs[name])
    for tab in (content, tabs["Frank"]):
        S.lab("herdr", "tab", "set-role", tab, "agent")
    S.app("start")
    S.cmd({"cmd": "activate"})
    wait(lambda s: s.get("window_key") is True, 10)
    S.cmd({"cmd": "select", "tab": content})
    wait(lambda s: s.get("selected_tab") == content)
    S.cmd({"cmd": "type", "text": "ls /"})
    S.cmd({"cmd": "key", "key": "return"})
    time.sleep(1)
    shoot("sidebar-terminal")
    S.cmd({"cmd": "docs", "open": True})
    time.sleep(0.6)
    shoot("docs")
    S.cmd({"cmd": "docs", "open": False})
    S.cmd({"cmd": "switcher", "open": True, "query": "or"})
    time.sleep(0.6)
    shoot("switcher")
    S.cmd({"cmd": "switcher", "open": False})


if __name__ == "__main__":
    try:
        main()
    finally:
        S.app("stop")
        S.lab("down")
