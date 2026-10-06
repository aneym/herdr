#!/usr/bin/env python3
"""The pin at the top right of the pane header pins the chat last in PINNED, or unpins it.

Alex, 2026-10-06: "a button to pin new tabs top right next to terminal/chat pane header".
Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 scripts/check_cap_pin.py.
The clicks go through NSApp.sendEvent at the pin's centre (the click hook, target cap_pin).
Checks, on a chat split in two panes:
  - only the right pane's header carries the pin, and it reads the server's pin fact;
  - a click pins the chat last in PINNED and in the server's pin order (after the agent pin
    and the pin already there), and the header shows it pinned;
  - a pin change made elsewhere (the CLI) reaches the header without a click;
  - a click on a pinned chat unpins it on the server.
Writes checks/CAP-PIN.txt and before/after shots.
"""
import json
import os
import pathlib
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_cap_pin requires HERDR_SHELL_SPACE=1; host launch is forbidden")
# Short name: the Space bridge's forwarded socket path must stay under 104 bytes.
os.environ["SHELL_LAB"] = "shellspike-cp"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/CAP-PIN.txt")
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


def pin_caps(state):
    """Caps that carry the pin, as (pane id, pinned)."""
    return [(c["id"], c["pinned"]) for c in state.get("pane_caps", []) if c.get("pinned") is not None]


def right_cap(state):
    caps = state.get("pane_caps", [])
    return max(caps, key=lambda c: c["frame"][0])["id"] if len(caps) == 2 else None


def pin_order(*workspaces):
    tabs = [tab for ws in workspaces for tab in api("tab", "list", "--workspace", ws)["tabs"]]
    return [t["tab_id"] for t in sorted((t for t in tabs if t.get("pin_index") is not None), key=lambda t: t["pin_index"])]


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
    made = api("tab", "create", "--workspace", ws, "--label", "beta", "--no-focus")
    b = made["tab"]["tab_id"]
    S.lab("herdr", "pane", "split", made["root_pane"]["pane_id"], "--direction", "right")
    agent = api("tab", "create", "--workspace", elsewhere, "--label", "lead", "--no-focus")["tab"]["tab_id"]
    for tab in (a, agent):
        S.lab("herdr", "tab", "pin", tab)
    S.lab("herdr", "tab", "set-role", agent, "agent")
    S.app("start")
    # SwiftUI drops synthesized mouse events on a window that is not key; the Space is the
    # app's own desktop, so bringing it front takes nothing from anyone.
    S.cmd({"cmd": "activate"})
    wait(lambda s: s.get("window_key") is True, timeout=10)
    S.cmd({"cmd": "select", "tab": b})
    state = wait(lambda s: s.get("selected_tab") == b and len(s.get("pane_caps", [])) == 2 and pin_caps(s))
    check("only the right pane's header has the pin, unpinned",
          pin_caps(state) == [(right_cap(state), False)], f"{pin_caps(state)} right={right_cap(state)}")
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/CAP-PIN-before.png")})

    S.cmd({"cmd": "click", "target": "cap_pin"})
    state = wait(lambda s: pinned_rows(s)[-1:] == [b] and [p for _, p in pin_caps(s)] == [True])
    order = pin_order(ws, elsewhere)
    check("a click pins it last in the server's pin order", order == [agent, a, b], str(order))
    check("it is last in PINNED", pinned_rows(state) == [a, b], str(pinned_rows(state)))
    check("the header shows it pinned", [p for _, p in pin_caps(state)] == [True], str(pin_caps(state)))
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/CAP-PIN-after.png")})

    S.lab("herdr", "tab", "unpin", b)
    state = wait(lambda s: [p for _, p in pin_caps(s)] == [False])
    check("an unpin elsewhere reaches the header", [p for _, p in pin_caps(state)] == [False], str(pin_caps(state)))
    S.lab("herdr", "tab", "pin", b)
    state = wait(lambda s: [p for _, p in pin_caps(s)] == [True])
    check("a pin elsewhere reaches the header", [p for _, p in pin_caps(state)] == [True], str(pin_caps(state)))

    S.cmd({"cmd": "click", "target": "cap_pin"})
    state = wait(lambda s: [p for _, p in pin_caps(s)] == [False] and b not in pinned_rows(s))
    order = pin_order(ws, elsewhere)
    check("a click on a pinned chat unpins it on the server", order == [agent, a], str(order))
    check("the header shows it unpinned", [p for _, p in pin_caps(state)] == [False], str(pin_caps(state)))

    S.app("stop")
    S.lab("down")
    pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
