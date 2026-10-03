#!/usr/bin/env python3
"""P27 check: chords Alex uses in Ghostty herdr do the same thing in Herdr Shell.

  python3 scripts/check_p27.py [--out checks/P27.txt]

Lab `shellspike-p27`. Four tabs: an idle lane, a blocked lane, a finished lane
(phase=done; `pane report-agent` has no done state), and a blocked lane that is
parked. Asserts from the state dump and from the lab herdr.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-p27"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LABDIR = os.path.expanduser(f"~/.cache/herdr-build/{os.environ['SHELL_LAB']}")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
FIX = os.path.join(LABDIR, "fixtures")
os.makedirs(FIX, exist_ok=True)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
CHK = os.path.dirname(S.OUT) if "--out" in sys.argv else os.path.join(D0, "checks")
if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "P27.txt")


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def herdr(*a):
    return S.lab("herdr", *a)


def jherdr(*a):
    return json.loads(herdr(*a))["result"]


def wait_state(pred, timeout=25):
    t0 = time.time()
    last = None
    while time.time() - t0 < timeout:
        try:
            last = S.state()
        except SystemExit:
            time.sleep(0.2)
            continue
        if last is not None and pred(last):
            return last
        time.sleep(0.2)
    return last


def act(name):
    S.cmd({"cmd": "action", "name": name})


def visible_tabs(s):
    """Sidebar rows Alex sees, skipping the Focus duplicate of a tab."""
    out = []
    for line in s.get("sidebar_lines", []):
        tab = line.get("tab")
        if not tab or str(line["id"]).startswith("focus:"):
            continue
        if tab not in out:
            out.append(tab)
    return out


def host_x(s):
    m = re.match(r"\{\{(-?\d+(?:\.\d+)?)\s*,", s.get("host_frame") or "")
    return float(m.group(1)) if m else None


def shot(name):
    png = os.path.join(CHK, name)
    if os.path.exists(png):
        os.unlink(png)
    S.cmd({"cmd": "shot", "out": png})
    for _ in range(80):
        if os.path.exists(png) and os.path.getsize(png) > 1000:
            break
        time.sleep(0.1)
    check(f"screenshot checks/{name}", os.path.exists(png) and os.path.getsize(png) > 1000, png)


def prompt(pane):
    for _ in range(200):
        if "%" in herdr("pane", "read", pane, "--source", "visible"):
            return
        time.sleep(0.05)


def report(pane, state, **tokens):
    prompt(pane)
    herdr("pane", "report-agent", pane, "--source", "spike", "--agent", "claude", "--state", state)
    if tokens:
        args = ["pane", "report-metadata", pane, "--source", "spike"]
        for k, v in tokens.items():
            args += ["--token", f"{k}={v}"]
        herdr(*args)


def main():
    say(f"HerdrShell P27 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    real = subprocess.run(["python3", os.path.join(D0, "scripts", "lab.py"), "env"], capture_output=True, text=True).stdout
    home = next(l.split("=", 1)[1] for l in real.splitlines() if l.startswith("HOME="))
    os.makedirs(os.path.join(LABDIR, "bin"), exist_ok=True)
    link = os.path.join(LABDIR, "bin", "herdr")
    if os.path.lexists(link):
        os.unlink(link)
    os.symlink(os.path.join(os.path.dirname(home), "bin", "herdr"), link)
    subprocess.run(["defaults", "delete", f"herdr.shell.dev.{os.environ['SHELL_LAB']}"], capture_output=True)

    old = jherdr("workspace", "list")["workspaces"]
    ws = jherdr("workspace", "create", "--label", "factory-space", "--cwd", "/tmp", "--no-focus")
    fw = ws["workspace"]["workspace_id"]
    for w in old:
        herdr("workspace", "close", w["workspace_id"])

    def tab(label):
        r = jherdr("tab", "create", "--workspace", fw, "--label", label, "--cwd", "/tmp", "--no-focus")
        return r["tab"]["tab_id"], r["root_pane"]["pane_id"]

    orch, orch_pane = ws["tab"]["tab_id"], ws["root_pane"]["pane_id"]
    herdr("tab", "rename", orch, "rails orchestrator")
    idle, idle_pane = tab("idle lane")
    blocked, blocked_pane = tab("blocked lane")
    done, done_pane = tab("done lane")
    parked, parked_pane = tab("parked lane")

    report(orch_pane, "working", kind="orchestrator")
    report(idle_pane, "idle", kind="lane")
    report(blocked_pane, "blocked", kind="lane")
    report(done_pane, "idle", kind="lane", phase="done")
    report(parked_pane, "blocked", kind="lane")

    def lane(t, name, kind="lane", section="implementing"):
        return {"tab": t, "name": name, "label": name, "kind": kind, "goal": None, "goal_area": "factory infra",
                "section": section, "section_source": "project", "scope_url": None, "review_url": None, "mode": None}

    lanes = {"version": 1, "generated_at": "2026-10-03T00:00:00Z", "lanes": [
        lane(orch, "rails orchestrator", "orchestrator", "orchestrator"),
        lane(idle, "idle lane"),
        lane(blocked, "blocked lane"),
        lane(done, "done lane"),
        lane(parked, "parked lane"),
    ]}
    areas = {"version": 1,
             "areas": [{"id": "factory", "name": "factory", "color": "#5AA9FF"}],
             "tabs": {orch: {"area": "factory", "role": "top"}},
             "spaces": {fw: "factory"}, "goal_area": {"factory infra": "factory"}, "goal": {}}
    modes = {"version": 1, "tabs": {
        parked: {"mode": "parked", "at": "2026-10-02T20:25:04.785Z", "by": "p27", "note": "skip me"},
    }}
    for name, doc in (("lanes.json", lanes), ("areas.json", areas), ("modes.json", modes)):
        with open(os.path.join(FIX, name), "w") as f:
            json.dump(doc, f)
    os.environ.update({
        "HERDR_LANES_PATH": os.path.join(FIX, "lanes.json"),
        "HERDR_AREAS_PATH": os.path.join(FIX, "areas.json"),
        "CONTROL_MODES": os.path.join(FIX, "modes.json"),
    })

    built = os.path.join(D0, ".build", "release", "HerdrShell")
    shutil.copy2(built, APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start: {S.app('start').strip()}")
    s = wait_state(lambda s: blocked in s.get("attention_order", []) and done in s.get("attention_order", [])
                   and parked not in s.get("attention_order", []), 40)
    check("attention order is blocked then done, parked left out",
          s is not None and s.get("attention_order") == [blocked, done],
          None if s is None else str(s.get("attention_order")))
    if s is None:
        return

    S.cmd({"cmd": "select", "tab": idle})
    s = wait_state(lambda s: s.get("selected_tab") == idle, 10)
    check("starts on the idle lane", s is not None and s.get("selected_tab") == idle)

    act("next_attention")
    s = wait_state(lambda s: s.get("selected_tab") == blocked, 10)
    check("cmd+e selects the blocked tab", s is not None and s.get("selected_tab") == blocked,
          None if s is None else s.get("selected_tab"))
    act("next_attention")
    s = wait_state(lambda s: s.get("selected_tab") == done, 10)
    check("cmd+e then selects the done tab", s is not None and s.get("selected_tab") == done,
          None if s is None else s.get("selected_tab"))
    act("next_attention")
    s = wait_state(lambda s: s.get("selected_tab") == blocked, 10)
    check("cmd+e wraps to blocked, skipping the parked tab",
          s is not None and s.get("selected_tab") == blocked and parked not in (s or {}).get("attention_order", []),
          None if s is None else s.get("selected_tab"))

    herdr("pane", "report-agent", orch_pane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    s = wait_state(lambda s: s.get("attention_latest") == orch, 20)
    check("a blocked transition is recorded", s is not None and s.get("attention_latest") == orch,
          None if s is None else str(s.get("attention_latest")))
    act("attention_jump")
    s = wait_state(lambda s: s.get("selected_tab") == orch, 10)
    check("cmd+o selects the latest transition", s is not None and s.get("selected_tab") == orch,
          None if s is None else s.get("selected_tab"))

    S.cmd({"cmd": "select", "tab": idle})
    wait_state(lambda s: s.get("selected_tab") == idle and s.get("focused_pane") == idle_pane, 10)
    before = len(jherdr("pane", "list")["panes"])
    S.cmd({"cmd": "split", "direction": "right"})
    s = wait_state(lambda s: len(jherdr("pane", "list")["panes"]) == before + 1, 15)
    check("split adds a pane", s is not None, f"panes {before} -> {len(jherdr('pane', 'list')['panes'])}")
    act("zoom_pane")
    s = wait_state(lambda s: jherdr("pane", "layout", "--pane", idle_pane)["layout"]["zoomed"] is True, 15)
    lay = jherdr("pane", "layout", "--pane", idle_pane)["layout"]
    wide = any(p["rect"]["width"] == lay["area"]["width"] for p in lay["panes"])
    check("zoom toggles on (layout zoomed, pane fills the area)",
          lay["zoomed"] is True and wide, f"zoomed={lay['zoomed']}")
    act("zoom_pane")
    s = wait_state(lambda s: jherdr("pane", "layout", "--pane", idle_pane)["layout"]["zoomed"] is False, 15)
    check("zoom toggles off", s is not None and jherdr("pane", "layout", "--pane", idle_pane)["layout"]["zoomed"] is False)

    mid = len(jherdr("pane", "list")["panes"])
    act("close_pane")
    s = wait_state(lambda s: len(jherdr("pane", "list")["panes"]) == mid - 1, 15)
    after = len(jherdr("pane", "list")["panes"])
    check("cmd+w closes a split pane (pane list drops by 1)", after == mid - 1, f"{mid} -> {after}")

    s = S.state()
    was = s.get("sidebar_visible")
    x0 = host_x(s)
    act("toggle_sidebar")
    s = wait_state(lambda s: s.get("sidebar_visible") is not was, 10)
    x1 = host_x(s) if s else None
    check("cmd+b hides the sidebar and the pane host takes the width",
          s is not None and s.get("sidebar_visible") is False and x0 is not None and x1 is not None and x1 < x0,
          f"visible={None if s is None else s.get('sidebar_visible')} host {x0} -> {x1}")
    shot("P27-sidebar-hidden.png")
    act("toggle_sidebar")
    s = wait_state(lambda s: s.get("sidebar_visible") is True, 10)
    check("cmd+b shows the sidebar again", s is not None and s.get("sidebar_visible") is True)

    s = wait_state(lambda s: len(visible_tabs(s)) >= 2, 10)
    tabs = visible_tabs(s) if s else []
    if len(tabs) >= 2:
        S.cmd({"cmd": "select", "tab": tabs[0]})
        wait_state(lambda s: s.get("selected_tab") == tabs[0], 10)
        act("agent_list_down")
        s = wait_state(lambda s: s.get("selected_tab") == tabs[1], 10)
        check("cmd+opt+k selects the next visible row",
              s is not None and s.get("selected_tab") == tabs[1],
              f"want {tabs[1]} got {None if s is None else s.get('selected_tab')} order={tabs}")
    else:
        check("cmd+opt+k selects the next visible row", False, f"rows={tabs}")

    S.cmd({"cmd": "rename", "tab": idle, "label": "renamed lane"})
    renamed = None
    for _ in range(40):
        try:
            renamed = jherdr("tab", "get", idle)["tab"]["label"]
        except Exception:
            renamed = None
        if renamed == "renamed lane":
            break
        time.sleep(0.25)
    check("rename changes the lab tab label", renamed == "renamed lane", str(renamed))


if __name__ == "__main__":
    code = 1
    try:
        main()
        code = 1 if failures else 0
    except Exception:
        import traceback
        traceback.print_exc()
        say("exception during the check")
        code = 1
    S.app("stop")
    time.sleep(0.4)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if code == 0 else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(S.OUT), exist_ok=True)
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(code)
