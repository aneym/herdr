#!/usr/bin/env python3
"""Chat finds the Claude session of a pane when the app is launched like Finder launches it.

  HERDR_SHELL_SPACE=1 python3 scripts/check_chat_session.py [--out checks/chat-session.txt]

Finder and launchd start the app with PATH=/usr/bin:/bin:/usr/sbin:/sbin, which has no
herdr on it. The chat asked `herdr agent get` through a bare `herdr`, so every live
Claude pane's chat read "No Claude session in this pane" (Alex, 2026-10-06). The lab
launch put the lab binary on PATH and hid it. This check launches the app with that
PATH, opens Chat on a lab pane reported as a Claude session, and waits for the chat to
show the session's state and its transcript.
"""
import json
import os
import shlex
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-cs"
os.environ["SHELL_APP_PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
OUT = os.path.join(S.D, "checks", "chat-session.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def finish():
    S.check_front(check)
    S.app("stop")
    time.sleep(0.4)
    say(f"lab down: {S.lab('down').strip()}")
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


def wait_state(pred, timeout=20):
    t0, last = time.time(), None
    while time.time() - t0 < timeout:
        try:
            last = S.state()
        except SystemExit:
            time.sleep(0.2)
            continue
        if pred(last):
            return last, True
        time.sleep(0.2)
    return last, False


def chat(st, pane):
    return next((c for c in (st or {}).get("chats", []) if c["id"] == pane), {})


def main():
    say(f"HerdrShell chat session check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.4)
    S.lab("up")
    env = dict(l.split("=", 1) for l in S.lab("env").splitlines() if "=" in l)
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    tab = next(t["tab_id"] for t in snap["tabs"] if t["label"] == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == tab)
    pane = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
    for _ in range(80):
        if "%" in S.lab("herdr", "pane", "read", pane, "--source", "visible"):
            break
        time.sleep(0.05)
    session = "cs-" + os.urandom(4).hex()
    S.lab("herdr", "pane", "report-agent", pane, "--source", "spike", "--agent", "claude", "--state", "idle")
    S.lab("herdr", "pane", "report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude",
          "--agent-session-id", session)
    agent = S.herdr_json("agent", "get", pane)["result"]["agent"]
    check("lab pane reports a Claude session", (agent.get("agent_session") or {}).get("value") == session, str(agent))
    cwd = agent.get("cwd") or ""
    encoded = "".join(c if c.isascii() and c.isalnum() else "-" for c in cwd)
    # Enough lines to scroll, so the screenshot shows whether the transcript stays below the pane's cap.
    record = "\n".join(json.dumps({"uuid": f"m{i}", "type": "assistant",
                                   "message": {"content": [{"type": "text", "text": f"Earlier reply {i}."}]}})
                        for i in range(60)) + "\n" + json.dumps({"uuid": "hello", "type": "assistant",
                                                                 "message": {"content": [{"type": "text", "text": "Lab session ready."}]}})
    # The app reads ~/.claude of the machine it runs on: the lab HOME here, the guest's in a Space.
    if S.SPACE:
        transcript = f"/Users/lume/.claude/projects/{encoded}/{session}.jsonl"
        S.space("exec", f"mkdir -p {shlex.quote(os.path.dirname(transcript))} && printf '%s\\n' "
                f"{shlex.quote(record)} > {shlex.quote(transcript)}")
    else:
        transcript = os.path.join(env["HOME"], ".claude", "projects", encoded, session + ".jsonl")
        os.makedirs(os.path.dirname(transcript), exist_ok=True)
        with open(transcript, "w") as f:
            f.write(record + "\n")
    say(f"pane {pane} session {session} cwd {cwd}; app PATH {os.environ['SHELL_APP_PATH']}")

    say(f"app start: {S.app('start').strip()}")
    S.cmd({"cmd": "select", "tab": tab})
    S.cmd({"cmd": "pane_mode", "id": pane, "mode": "chat"})
    st, ok = wait_state(lambda s: chat(s, pane).get("agent_state") == "idle"
                        and "hello:0" in chat(s, pane).get("items", []), 20)
    check("chat shows the session's state and transcript, not 'No Claude session'", ok,
          json.dumps({k: v for k, v in chat(st, pane).items() if k != "items"} | {"last_item": (chat(st, pane).get("items") or [""])[-1]}))
    png = os.path.splitext(OUT)[0] + ".png"
    if os.path.exists(png):
        os.unlink(png)
    S.cmd({"cmd": "shot", "out": png})
    for _ in range(40):
        if os.path.exists(png) and os.path.getsize(png) > 1000:
            break
        time.sleep(0.1)
    say(f"shot: {png}")
    finish()


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        say(f"FAIL {exc!r}")
        failures.append(str(exc))
        finish()
