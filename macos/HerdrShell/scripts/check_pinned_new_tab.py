#!/usr/bin/env python3
"""The + on the PINNED header makes a new chat, pinned at the end of PINNED and focused.

Alex, 2026-10-06: "i need a button next to pinned to make a new tab that's pinned please".
Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 scripts/check_pinned_new_tab.py.
The click goes through NSApp.sendEvent at the + frame (the click hook, target pinned_plus).
Checks:
  - a click adds one chat in the focused chat's space, last in PINNED and in the server's
    pin order (whose index counts the agent pins ahead of it), and selects it;
  - with a restored selection on another machine that has not answered yet, a click waits for
    that machine and makes the chat in the selected chat's space there, not in a local space.
    The machine (m1) is this lab again, through a guest socket that appears after the click.
Writes checks/PINNED-NEW-TAB.txt and before/after shots.
"""
import json
import os
import pathlib
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_pinned_new_tab requires HERDR_SHELL_SPACE=1; host launch is forbidden")
# Short name: the Space bridge's forwarded socket path must stay under 104 bytes.
os.environ["SHELL_LAB"] = "shellspike-pn"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/PINNED-NEW-TAB.txt")
lines = []
failures = []


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


def pinned_rows(state):
    return [row.split("|")[1][len("pinned:"):] for row in state.get("spaces_rows", []) if row.split("|")[1].startswith("pinned:")]


def server_tabs(*workspaces):
    return [tab for ws in workspaces for tab in api("tab", "list", "--workspace", ws)["tabs"]]


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    home = api("workspace", "create", "--label", "home", "--no-focus")
    other = api("workspace", "create", "--label", "other", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws, elsewhere = home["workspace"]["workspace_id"], other["workspace"]["workspace_id"]
    a = home["tab"]["tab_id"]
    b = api("tab", "create", "--workspace", ws, "--label", "beta", "--no-focus")["tab"]["tab_id"]
    c = other["tab"]["tab_id"]
    agent = api("tab", "create", "--workspace", elsewhere, "--label", "lead", "--no-focus")["tab"]["tab_id"]
    for tab in (a, c, agent):
        S.lab("herdr", "tab", "pin", tab)
    S.lab("herdr", "tab", "set-role", agent, "agent")
    S.app("start")
    # SwiftUI drops synthesized mouse events on a window that is not key; the Space is the
    # app's own desktop, so bringing it front takes nothing from anyone.
    S.cmd({"cmd": "activate"})
    wait(lambda s: s.get("window_key") is True, timeout=10)
    S.cmd({"cmd": "select", "tab": b})
    state = wait(lambda s: s.get("selected_tab") == b and pinned_rows(s) == [a, c])
    check("pins listed before the click", pinned_rows(state) == [a, c], str(pinned_rows(state)))
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/PINNED-NEW-TAB-before.png")})

    before = {t["tab_id"] for t in server_tabs(ws, elsewhere)}
    S.cmd({"cmd": "click", "target": "pinned_plus"})
    state = wait(lambda s: len(pinned_rows(s)) == 3 and s.get("selected_tab") == pinned_rows(s)[-1])
    rows = pinned_rows(state)
    made = [t for t in server_tabs(ws, elsewhere) if t["tab_id"] not in before]
    check("one new chat on the server", len(made) == 1, str(made))
    new = made[0]["tab_id"] if made else None
    check("it is in the focused chat's space", bool(made) and made[0]["workspace_id"] == ws, str(made))
    check("it is last in PINNED", rows == [a, c, new], str(rows))
    pins = sorted((t for t in server_tabs(ws, elsewhere) if t.get("pin_index") is not None), key=lambda t: t["pin_index"])
    order = [t["tab_id"] for t in pins]
    check("server pin order ends with it", order == [agent, a, c, new], str(order))
    check("it is focused", state.get("selected_tab") == new, str(state.get("selected_tab")))
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/PINNED-NEW-TAB-after.png")})

    S.app("stop")
    m1 = "/Users/lume/.herdr-space/m1.sock"
    S.space("exec", f"rm -f {m1}")
    machines = pathlib.Path(S.LAB) / "machines.json"
    machines.write_text(json.dumps({"machines": [{"name": "m1", "socket": m1}]}))
    os.environ["HERDR_SHELL_MACHINES"] = str(machines)
    S.app("start")
    S.cmd({"cmd": "activate"})
    wait(lambda s: s.get("window_key") is True, timeout=10)
    S.cmd({"cmd": "select", "tab": f"m1/{c}"})
    state = wait(lambda s: s.get("selected_tab") == f"m1/{c}", timeout=10)
    check("a selection on a machine that has not answered is kept", state.get("selected_tab") == f"m1/{c}",
          str(state.get("selected_tab")))
    before = {t["tab_id"] for t in server_tabs(ws, elsewhere)}
    S.cmd({"cmd": "click", "target": "pinned_plus"})
    time.sleep(2)
    early = [t for t in server_tabs(ws, elsewhere) if t["tab_id"] not in before]
    check("nothing is made before that machine answers", early == [], str(early))
    S.space("exec", f"ln -sf /Users/lume/.herdr-space/herdr.sock {m1}")
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and not [t for t in server_tabs(ws, elsewhere) if t["tab_id"] not in before]:
        time.sleep(0.3)
    made = [t for t in server_tabs(ws, elsewhere) if t["tab_id"] not in before]
    remote = made[0]["tab_id"] if made else None
    check("once it answers, one chat is made in the selected chat's space there",
          len(made) == 1 and made[0]["workspace_id"] == elsewhere, str(made))
    pins = sorted((t for t in server_tabs(ws, elsewhere) if t.get("pin_index") is not None), key=lambda t: t["pin_index"])
    order = [t["tab_id"] for t in pins]
    check("it is pinned last on that machine", order == [agent, a, c, new, remote], str(order))
    state = wait(lambda s: s.get("selected_tab") == f"m1/{remote}", timeout=15)
    check("it is focused as that machine's chat", state.get("selected_tab") == f"m1/{remote}", str(state.get("selected_tab")))

    S.app("stop")
    S.space("exec", f"rm -f {m1}")
    S.lab("down")
    pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
