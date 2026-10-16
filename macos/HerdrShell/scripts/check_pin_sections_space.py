#!/usr/bin/env python3
import json
import os
import pathlib
import sys
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_pin_sections_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-pinsec"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.setdefault("HERDR_SPACE_OWNER", "check-pin-sections")
ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/PIN-SECTIONS-SPACE.txt")
SHOTS = pathlib.Path(os.environ.get("PIN_SECTIONS_SHOTS", str(ROOT / "checks")))
SECTIONS = ["Needs you", "Northstar: building", "Later"]
lines, failures = [], []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=10):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    return state


def pinned_block(state):
    rows = [r.split("|") for r in state.get("spaces_rows", [])]
    out, inside = [], False
    for row in rows:
        kind, rid = row[0], row[1]
        if rid == "pinned":
            inside = True
            continue
        if inside and (kind == "section" and not rid.startswith("pinsection:") or kind in ("space", "goal", "title")):
            break
        if inside:
            out.append(rid if kind != "section" else f"{rid}|{row[3]}|{row[6]}")
    return out


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "rails", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws, first = made["workspace"]["workspace_id"], made["tab"]["tab_id"]
    S.lab("herdr", "tab", "rename", first, "rails orchestrator")
    names = ["scope review", "runtime chat", "billing build", "deploy watch", "old notes"]
    tabs = [first] + [api("tab", "create", "--workspace", ws, "--label", n, "--no-focus")["tab"]["tab_id"] for n in names]
    for tab in tabs:
        S.lab("herdr", "tab", "pin", tab)
    S.lab("herdr", "pin-section", "create", "Needs you")
    S.lab("herdr", "pin-section", "create", "Later")
    S.lab("herdr", "pin-section", "create", "Northstar: building", "--after", "Needs you")
    listed = json.loads(S.lab("herdr", "pin-section", "list", "--json"))
    check("CLI: pin-section create orders sections with --after",
          [s["name"] for s in listed["sections"]] == SECTIONS, json.dumps(listed))
    S.lab("herdr", "tab", "pin-section", tabs[1], "Needs you")
    S.lab("herdr", "tab", "pin-section", tabs[2], "Northstar: building")
    S.lab("herdr", "tab", "pin-section", tabs[3], "Northstar: building")
    S.lab("herdr", "tab", "pin-section", tabs[4], "Needs you")
    S.lab("herdr", "tab", "unpin", tabs[5])
    S.lab("herdr", "tab", "pin", tabs[5], "--section", "Later")
    text = S.lab("herdr", "pin-section", "list").strip().splitlines()
    check("CLI: pin-section list prints unsectioned pins, then each section's tabs",
          text == [f"(unsectioned) {tabs[0]}", f"Needs you: {tabs[1]} {tabs[4]}",
                   f"Northstar: building: {tabs[2]} {tabs[3]}", f"Later: {tabs[5]}"], json.dumps(text))
    order = [t["tab_id"] for t in sorted((t for t in api("tab", "list")["tabs"] if t.get("pin_index") is not None),
                                          key=lambda t: t["pin_index"])]
    check("the server's pin order follows the sections (Cmd+1..9 match the drawn order)",
          order == [tabs[0], tabs[1], tabs[4], tabs[2], tabs[3], tabs[5]], json.dumps(order))
    moved = S.lab("herdr", "tab", "pin-move", tabs[4], "1")
    pins = {t["tab_id"]: t.get("pin_index") for t in api("tab", "list")["tabs"]}
    check("pin-move stays inside the tab's section", pins[tabs[4]] == 1 and pins[tabs[1]] == 2, moved + json.dumps(pins))

    S.app("start")
    S.cmd({"cmd": "activate"})
    want = ["pinned:" + tabs[0],
            "pinsection:Needs you|open|NEEDS YOU", "pinned:" + tabs[4], "pinned:" + tabs[1],
            "pinsection:Northstar: building|open|NORTHSTAR: BUILDING", "pinned:" + tabs[2], "pinned:" + tabs[3],
            "pinsection:Later|open|LATER", "pinned:" + tabs[5]]
    state = wait(lambda s: pinned_block(s) == want, 30)
    check("Shell draws unsectioned pins first, then each named section in order with its pins",
          pinned_block(state) == want, json.dumps(pinned_block(state)))
    S.cmd({"cmd": "appearance", "mode": "light"})
    wait(lambda s: s.get("theme", {}).get("effective") == "light")
    time.sleep(0.8)
    png = SHOTS / "PIN-SECTIONS-SPACE-light.png"
    png.parent.mkdir(parents=True, exist_ok=True)
    S.cmd({"cmd": "shot", "out": str(png), "scale": 2})
    time.sleep(1.5)
    check("light Shell window shot written", png.exists() and png.stat().st_size > 0)

    S.cmd({"cmd": "spaces_click", "row": "pinsection:Later"})
    state = wait(lambda s: "pinned:" + tabs[5] not in pinned_block(s), 4)
    check("a section header folds its pins and shows their count",
          pinned_block(state)[-1] == "pinsection:Later|closed|LATER" and "pinned:" + tabs[5] not in pinned_block(state),
          json.dumps(pinned_block(state)))
    S.app("stop")
    S.app("start")
    state = wait(lambda s: "pinsection:Later|closed|LATER" in pinned_block(s), 30)
    check("the fold is remembered across a restart", "pinned:" + tabs[5] not in pinned_block(state),
          json.dumps(pinned_block(state)))
    S.cmd({"cmd": "spaces_click", "row": "pinsection:Later"})

    S.lab("herdr", "pin-section", "rename", "Needs you", "Needs Alex")
    S.lab("herdr", "pin-section", "move", "Later", "--index", "0")
    state = wait(lambda s: any(r.startswith("pinsection:Needs Alex") for r in pinned_block(s))
                 and pinned_block(s)[1].startswith("pinsection:Later"), 10)
    check("rename and move reach the Shell from the server", pinned_block(state)[1:3] == ["pinsection:Later|open|LATER", "pinned:" + tabs[5]]
          and "pinsection:Needs Alex|open|NEEDS ALEX" in pinned_block(state), json.dumps(pinned_block(state)))
    S.lab("herdr", "pin-section", "delete", "Needs Alex")
    state = wait(lambda s: not any(r.startswith("pinsection:Needs Alex") for r in pinned_block(s)), 10)
    block = pinned_block(state)
    check("deleting a section keeps its tabs pinned, unsectioned", block[:3] == ["pinned:" + tabs[0], "pinned:" + tabs[4], "pinned:" + tabs[1]],
          json.dumps(block))


if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        check("scenario completed", False, repr(exc)[:400])
    finally:
        try:
            S.app("stop")
            S.lab("down")
        finally:
            lines.append(f"{sum(l.startswith('[PASS]') for l in lines)} passed; {len(failures)} failed")
            pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
            pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))
