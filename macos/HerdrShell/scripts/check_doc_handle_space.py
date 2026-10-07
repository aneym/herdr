#!/usr/bin/env python3
"""The docs width handle resizes on the first press in an inactive window, in the Cua Space.

Run: HERDR_SHELL_SPACE=1 python3 scripts/check_doc_handle_space.py.
Finder takes the front inside the Space, so Herdr Shell is inactive and its window not key; the
drag then goes through window.sendEvent, where AppKit applies its first-mouse rule as for a
physical click. A handle without acceptsFirstMouse only activates the window and keeps its width.
A drag on the key window first is the control. Writes checks/DOC-HANDLE-SPACE.txt.
"""
import json
import os
import pathlib
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_doc_handle_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-dh"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/DOC-HANDLE-SPACE.txt")
lines, failures = [], []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=15):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    return state


def drawn(state):
    """The docs column's drawn width, so a hidden column never passes for a resized one."""
    return state.get("shell", {}).get("doc_frame_width", 0)


def drag(dx):
    S.cmd({"cmd": "drag_doc_handle", "dx": dx, "steps": 8, "interval": 0.05})
    time.sleep(0.3)
    return wait(lambda s: not s.get("drag_running"), 10)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "docs", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    tab = made["tab"]["tab_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "select", "tab": tab})
    # The column only shows with something on the tab's desk.
    S.lab("herdr", "desk", "open", "--tab", tab, "https://example.com/")
    S.cmd({"cmd": "docs", "open": True, "width": 510})
    state = wait(lambda s: s.get("window_key") is True and abs(drawn(s) - 510) < 1)
    check("docs drawn 510 wide in the key window", state.get("window_key") is True and abs(drawn(state) - 510) < 1,
          f"key={state.get('window_key')} drawn={drawn(state)} shell={json.dumps(state.get('shell', {}))[:200]}")

    state = drag(40)
    check("control: a drag on the key window narrows it by 40", abs(drawn(state) - 470) < 2, f"width={drawn(state)}")

    S.space("exec", "open -a Finder")
    state = wait(lambda s: s.get("app_active") is False and s.get("window_key") is False)
    check("Finder in front: Herdr Shell inactive, window not key",
          state.get("app_active") is False and state.get("window_key") is False,
          f"active={state.get('app_active')} key={state.get('window_key')}")
    state = drag(-80)
    check("the first press on the handle of an inactive window drags it 80 wider",
          abs(drawn(state) - 550) < 2, f"width={drawn(state)}")


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
