#!/usr/bin/env python3
"""Wrapped terminal URL hover/open and a bare long URL in a live Chat transcript.

Run: HERDR_SHELL_SPACE=1 python3 scripts/check_links_space.py.
Exercises libghostty's link detection through real pane output and mouse events,
not the static ChatLinks parser. Writes checks/LINKS-SPACE.txt and three shots.
"""
import json
import os
import pathlib
import shlex
import time
import uuid

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_links_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-ln"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/LINKS-SPACE.txt")
lines, failures = [], []
PREFIX = "https://example.com/"
URL = PREFIX + "a" * (260 - len(PREFIX))
OSC_URL = "https://example.com/osc8-target"


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def skip(name):
    line = "[SKIP] " + name
    print(line, flush=True)
    lines.append(line)


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


def surface(state, pane):
    return next((s for s in state.get("surfaces", []) if s["pane"] == pane), {})


def chat(state, pane):
    return next((c for c in state.get("chats", []) if c["id"] == pane), {})


def shot(name):
    S.cmd({"cmd": "shot", "out": str(ROOT / ("checks/" + name + ".png"))})


def chat_check(pane):
    # The established check_chat_session path reports a session on the lab pane and
    # writes only its synthetic transcript in the guest HOME; no real account is read.
    S.lab("herdr", "pane", "report-agent", pane, "--source", "spike", "--agent", "claude", "--state", "idle")
    session = "ln-" + uuid.uuid4().hex[:8]
    S.lab("herdr", "pane", "report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude",
          "--agent-session-id", session)
    agent = api("agent", "get", pane)["agent"]
    if (agent.get("agent_session") or {}).get("value") != session:
        check("pane reports the synthetic Chat session", False)
        return
    cwd = agent.get("cwd")
    if not cwd:
        skip("Chat transcript: agent.get returned no cwd; cannot locate the established transcript path")
        return
    encoded = "".join(c if c.isascii() and c.isalnum() else "-" for c in cwd)
    transcript = f"/Users/lume/.claude/projects/{encoded}/{session}.jsonl"
    record = json.dumps({"uuid": "long-link", "type": "assistant",
                         "message": {"content": [{"type": "text", "text": URL}]}})
    S.space("exec", f"mkdir -p {shlex.quote(os.path.dirname(transcript))} && printf '%s\\n' "
            f"{shlex.quote(record)} > {shlex.quote(transcript)}")
    S.cmd({"cmd": "pane_mode", "id": pane, "mode": "chat"})
    state = wait(lambda s: "long-link:0" in chat(s, pane).get("items", []))
    check("Chat opens and loads the assistant's bare 260-character URL",
          "long-link:0" in chat(state, pane).get("items", [])
          and any(c.get("id") == pane and c.get("chat") is True for c in state.get("pane_caps", [])))
    shot("LINKS-chat")


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "links", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    tab, pane = made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "select", "tab": tab})
    state = wait(lambda s: s.get("window_key") is True and surface(s, pane).get("cols", 0) > 0)
    check("app is key and terminal attached", state.get("window_key") is True and bool(surface(state, pane)))
    # Wait for the shell before entering the command. Clear/home after the command's
    # echo; sleep keeps prompts from joining the URL or displacing its physical rows.
    _, ready = S.wait_read(pane, lambda text: "%" in text, timeout=10)
    check("lab shell ready for output command", ready is not None)
    command = ("printf '\\033[2J\\033[H%s\\n\\033]8;;https://example.com/osc8-target\\033\\\\label"
               "\\033]8;;\\033\\\\\\n' " + shlex.quote(URL) + "; sleep 3600")
    S.lab("herdr", "pane", "run", pane, command)
    state = wait(lambda s: any(line.startswith(PREFIX) for line in surface(s, pane).get("visible_nonblank", []))
                 and "label" in surface(s, pane).get("visible_nonblank", []))
    shot("LINKS-terminal")
    cols = surface(state, pane).get("cols", 0)
    # pane.read visible retains physical blank rows, unlike visible_nonblank. Resolve
    # the continuation's row from terminal text/grid metadata, never screenshot pixels.
    visible = S.lab("herdr", "pane", "read", pane, "--source", "visible").splitlines()
    start = next((i for i, line in enumerate(visible) if line.startswith(PREFIX)), None)
    wrapped = (0 < cols < len(URL) and start is not None and start + 1 < len(visible)
               and visible[start].rstrip() == URL[:cols]
               and visible[start + 1].rstrip() == URL[cols:2 * cols])
    check("260-character URL is one logical line soft-wrapped into a second physical row", wrapped)
    check("OSC 8 label printed", "label" in [line.strip() for line in visible])
    label_row = next((i for i, line in enumerate(visible) if line.strip() == "label"), None)
    if wrapped:
        def hover(name, row, col, want):
            S.cmd({"cmd": "mouse", "pane": pane, "col": float(col), "row": float(row), "mods": ["cmd"],
                   "action": "move"})
            got = wait(lambda s: surface(s, pane).get("hovered_link") == want, 8)
            seen = surface(got, pane).get("hovered_link", "")
            check(name, seen == want, f"hovered_link={seen[:48]}... len={len(seen)}")
            return got

        def click(name, row, col, want, mods=("cmd",)):
            pointer = {"cmd": "mouse", "pane": pane, "col": float(col), "row": float(row), "mods": list(mods)}
            S.cmd({**pointer, "action": "move"})
            before = list(S.state().get("opened_urls", []))
            S.cmd({**pointer, "action": "down"})
            S.cmd({**pointer, "action": "up"})
            got = wait(lambda s: s.get("opened_urls", []) != before, 8 if want else 2)
            opened = got.get("opened_urls", [])
            if want:
                # Allow duplicate callbacks to arrive before asserting exact count.
                time.sleep(0.5)
                opened = S.state().get("opened_urls", [])
                check(name, opened == before + [want],
                      f"new opens={opened[len(before):]}")
            else:
                check(name, opened == before, f"opened {opened[len(before):]}")

        # Hover first: the shot shows the pointer state on the wrapped continuation.
        hover("Cmd-hover on URL first row reports the entire URL", start, 2, URL)
        if label_row is not None:
            hover("Cmd-hover on OSC 8 label reports its target", label_row, 2, OSC_URL)
        hover("Cmd-hover on URL second row reports the entire URL", start + 1, 2, URL)
        shot("LINKS-hover")
        S.cmd({"cmd": "mouse", "pane": pane, "col": 2.0, "row": float(start + 1), "action": "move"})
        state = wait(lambda s: surface(s, pane).get("hovered_link") == "", 5)
        check("moving without Cmd clears the hovered link", surface(state, pane).get("hovered_link") == "")
        click("Cmd-click on URL second row records the entire URL", start + 1, 2, "desk " + URL)
        click("Cmd-click on URL first row records the entire URL", start, 5, "desk " + URL)
        if label_row is not None:
            click("Cmd-click on OSC 8 label records its target", label_row, 2, "desk " + OSC_URL)
        click("Cmd-Shift-click opens the URL externally", start + 1, 2, URL, mods=("cmd", "shift"))
        click("plain click on the URL opens nothing", start + 1, 2, None, mods=())
        # This independently checks the existing hook/callback path, only after the
        # real click receipts above so simulation cannot supply them.
        before = list(S.state().get("opened_urls", []))
        S.cmd({"cmd": "open_url_sim", "url": OSC_URL})
        state = wait(lambda s: s.get("opened_urls", []) != before, 8)
        opened = state.get("opened_urls", [])
        check("open_url_sim records the OSC 8 target", opened != before and bool(opened) and opened[-1] == "desk " + OSC_URL)
    else:
        skip("wrapped-row mouse checks: physical continuation row was not established (see FAIL above)")
    chat_check(pane)


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
