#!/usr/bin/env python3
"""S6 (lane herdr-ui): each pane's header names that pane; none reads the old "Brief" placeholder.

Found 2026-10-13 by the lane's prod walker: five plain zsh panes in one tab, every header
"Brief" (Mac and PC Shell 19fd4ba6). The cap name follows the TUI border's order (agent title,
manual label, agent kind), then the terminal's own title, then the working folder.
Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 scripts/check_pane_header.py [--shots DIR].
One scratch tab on a lab server, four plain shells:
  - one renamed with `herdr pane rename` shows the label;
  - one that sets an OSC window title shows that title;
  - two in different folders show their folder names.
Writes checks/PANE-HEADER.txt; with --shots, light and dark shots of the tab.
"""
import json
import os
import pathlib
import sys
import tempfile
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_pane_header requires HERDR_SHELL_SPACE=1; host launch is forbidden")
# Short name: the Space bridge's forwarded socket path must stay under 104 bytes.
os.environ["SHELL_LAB"] = "shellspike-ph"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/PANE-HEADER.txt")
SHOTS = pathlib.Path(sys.argv[sys.argv.index("--shots") + 1]) if "--shots" in sys.argv else None
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


def names(state):
    return {c["id"]: c["name"] for c in state.get("pane_caps", [])}


def main():
    folders = pathlib.Path(tempfile.mkdtemp(prefix="s6-header-"))
    (folders / "alpha-folder").mkdir()
    (folders / "beta-folder").mkdir()
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    home = api("workspace", "create", "--label", "home", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    made = api("tab", "create", "--workspace", home["workspace"]["workspace_id"], "--label", "scratch", "--no-focus")
    tab, labeled = made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    titled = api("pane", "split", labeled, "--direction", "right")["pane"]["pane_id"]
    alpha = api("pane", "split", labeled, "--direction", "down", "--cwd", str(folders / "alpha-folder"))["pane"]["pane_id"]
    beta = api("pane", "split", titled, "--direction", "down", "--cwd", str(folders / "beta-folder"))["pane"]["pane_id"]
    S.lab("herdr", "pane", "rename", labeled, "server")
    # The sleep keeps the prompt from redrawing (and a theme from resetting) the title.
    S.lab("herdr", "pane", "send-text", titled, "printf '\\033]2;vim notes.md\\007'; sleep 600\n")
    want = {labeled: "server", titled: "vim notes.md", alpha: "alpha-folder", beta: "beta-folder"}

    S.app("start")
    S.cmd({"cmd": "select", "tab": tab})
    state = wait(lambda s: s.get("selected_tab") == tab and names(s) == want)
    got = names(state)
    check("four panes have four caps", len(got) == 4, json.dumps(got))
    check("a renamed pane's header shows its label", got.get(labeled) == "server", json.dumps(got))
    check("a pane's header shows its terminal title", got.get(titled) == "vim notes.md", json.dumps(got))
    check("plain shells show their folder", got.get(alpha) == "alpha-folder" and got.get(beta) == "beta-folder", json.dumps(got))
    check("no header reads Brief", "Brief" not in got.values(), json.dumps(got))

    if SHOTS:
        SHOTS.mkdir(parents=True, exist_ok=True)
        for mode in ("light", "dark"):
            S.cmd({"cmd": "appearance", "mode": mode})
            wait(lambda s: s.get("theme", {}).get("effective") == mode, timeout=10)
            time.sleep(0.5)
            S.cmd({"cmd": "shot", "out": str(SHOTS / f"mac-pane-header-{mode}.png")})
        S.cmd({"cmd": "appearance", "mode": "light"})

    S.app("stop")
    S.lab("down")
    pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
