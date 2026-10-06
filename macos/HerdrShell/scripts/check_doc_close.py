#!/usr/bin/env python3
"""Docs column: closed until asked for, and ⌘W closes the column, not the tab.

Run only in the Cua Space: HERDR_SHELL_SPACE=1 python3 scripts/check_doc_close.py.
Alex, 2026-10-06: "i pressed cmd w on the resume and brief, and it closed this tab instead
of that. by default, resume and brief never need to be opened in my view."

The guest starts with the old window-wide `docOpen` left on, as Alex's install had it. A lane
tab with RESUME.md and BRIEF.md opens with no column; ⌘\\ opens it; after a click on a doc tab
(which hands keys back to the pane) ⌘W closes the column and the tab keeps its pane.
"""
import json
import os
import pathlib
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_doc_close requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-docw"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
LAB = pathlib.Path.home() / ".cache/herdr-build/shellspike-docw"
LAB.mkdir(parents=True, exist_ok=True)
os.environ["HERDR_LANES_PATH"] = str(LAB / "lanes.json")
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/doc-close.txt")
lines = []
failures = []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" -- {detail}" if detail else "")
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


def shot(name):
    S.cmd({"cmd": "shot", "out": str(ROOT / f"checks/doc-close-{name}.png")})


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    domain = "herdr.shell.dev.shellspike-docw"
    S.space("exec", f"defaults delete {domain} 2>/dev/null; defaults write {domain} herdr.shell.docOpen -bool true")
    lane = "/Users/lume/.agent-rails/lanes/docs-close"
    S.space("exec", f"rm -rf {lane} && mkdir -p {lane}"
            f" && printf '# docs close RESUME\\n\\nstage: Ready for review\\n' > {lane}/RESUME.md"
            f" && printf '# docs close BRIEF\\n\\nA short brief.\\n' > {lane}/BRIEF.md")
    old = api("workspace", "list")["workspaces"]
    created = api("workspace", "create", "--label", "docs", "--cwd", "/tmp", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    tab = created["tab"]["tab_id"]
    pane = created["root_pane"]["pane_id"]
    # The lane name picks the project folder: ~/.agent-rails/lanes/docs-close in the guest.
    lanes = {"version": 1, "lanes": [{"tab": tab, "name": "docs close", "label": "docs close", "kind": "lane",
                                      "section": "implementing", "scope_url": None, "review_url": None}]}
    pathlib.Path(os.environ["HERDR_LANES_PATH"]).write_text(json.dumps(lanes))
    S.app("start")
    S.cmd({"cmd": "frame", "w": 1440, "h": 900})
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "select", "tab": tab})
    state = wait(lambda s: s.get("selected_tab") == tab and s.get("docs", {}).get("tabs") == ["RESUME", "BRIEF"], 40)
    check("the lane tab has RESUME and BRIEF", state.get("docs", {}).get("tabs") == ["RESUME", "BRIEF"],
          f"tabs={state.get('docs', {}).get('tabs')}")
    time.sleep(0.5)
    state = S.state()
    check("the tab opens with no docs column, though the old window-wide docOpen is on",
          state.get("docs_visible") is False and state["shell"]["doc_open"] is False,
          f"docs_visible={state.get('docs_visible')} doc_open={state['shell']['doc_open']}")
    shot("closed")

    S.key("\\", ["cmd"])
    state = wait(lambda s: s.get("docs_visible") is True and "docs close RESUME" in (s["docs"].get("text") or ""), 10)
    check("cmd+\\ opens the column on request", state.get("docs_visible") is True,
          f"docs_visible={state.get('docs_visible')} text={(state.get('docs', {}).get('text') or '')[:80]!r}")
    S.cmd({"cmd": "click", "target": "doc_tab", "label": "BRIEF"})
    state = wait(lambda s: "docs close BRIEF" in (s["docs"].get("text") or ""), 10)
    check("a click on the BRIEF tab shows BRIEF", state.get("docs", {}).get("active") == "BRIEF",
          f"active={state.get('docs', {}).get('active')}")
    shot("open")

    S.key("w", ["cmd"])
    state = wait(lambda s: s.get("docs_visible") is False, 5)
    time.sleep(1.0)
    panes = [p["pane_id"] for p in api("pane", "list")["panes"] if p.get("tab_id") == tab]
    check("cmd+w after a click in the column closes the column",
          state.get("docs_visible") is False and state["shell"]["doc_open"] is False,
          f"docs_visible={state.get('docs_visible')} doc_open={state['shell']['doc_open']}")
    check("cmd+w after a click in the column leaves the tab and its pane", panes == [pane], f"panes in tab={panes}")
    shot("after-cmd-w")

    S.app("stop")
    S.lab("down")
    pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))


if __name__ == "__main__":
    main()
