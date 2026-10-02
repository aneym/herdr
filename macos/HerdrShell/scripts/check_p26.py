#!/usr/bin/env python3
"""P26 check: parked tabs leave the areas and filters, sit in one Parked group, resume in one click.

  python3 scripts/check_p26.py [--out checks/P26.txt]

Lab `shellspike-parked`. Fixtures: lanes.json, areas.json and a modes.json (CONTROL_MODES)
with two parked tabs, one of them a blocked lane that would otherwise lead Needs you.
Park and Resume run the real herdr-lane (lane.js) against the fixture modes file; its
kinds registry is an empty lab dir and HERDR_KIND_BIN is /usr/bin/true, so nothing
outside the lab is written. Asserts from the state dump, a real click on Resume, and
the modes file on disk.
"""
import json
import os
import shutil
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-parked"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LABDIR = os.path.expanduser(f"~/.cache/herdr-build/{os.environ['SHELL_LAB']}")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
FIX = os.path.join(LABDIR, "fixtures")
os.makedirs(os.path.join(FIX, "workflows", "kinds"), exist_ok=True)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
CHK = os.path.dirname(S.OUT) if "--out" in sys.argv else os.path.join(D0, "checks")
if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "P26.txt")


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
        if pred(last):
            return last
        time.sleep(0.2)
    return last


def click(target, label=None):
    # Attach respawns in the lab can take key status; a click on a non-key window is dropped.
    S.cmd({"cmd": "activate"})
    time.sleep(0.2)
    body = {"cmd": "click", "target": target}
    if label is not None:
        body["label"] = label
    S.cmd(body)
    time.sleep(0.45)


def click_until(target, label, pred, tries=4):
    """A click on a window that just lost key status is dropped; click again until it lands."""
    s = None
    for _ in range(tries):
        click(target, label)
        s = wait_state(pred, 6)
        if s is not None and pred(s):
            return s
    return s


def live(s):
    """Titles drawn outside the Parked group and the Focus list."""
    return [l["title"] for l in s["sidebar_lines"]
            if l.get("tab") and not l["parked"] and not str(l["id"]).startswith("focus:")]


def parked_rows(s):
    return [l for l in s["sidebar_lines"] if l["parked"]]


def focus_titles(s):
    return [l["title"] for l in s["sidebar_lines"] if str(l["id"]).startswith("focus:")]


def modes_doc():
    with open(os.path.join(FIX, "modes.json")) as f:
        return json.load(f)


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


def main():
    say(f"HerdrShell P26 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    # run.sh passes --herdr $LAB/bin/herdr under the long lab name; lab.py keeps the server
    # under a short dir (socket length). Park/Resume run the herdr CLI, so link it.
    real = subprocess.run(["python3", os.path.join(D0, "scripts", "lab.py"), "env"], capture_output=True, text=True).stdout
    home = next(l.split("=", 1)[1] for l in real.splitlines() if l.startswith("HOME="))
    os.makedirs(os.path.join(LABDIR, "bin"), exist_ok=True)
    link = os.path.join(LABDIR, "bin", "herdr")
    if os.path.lexists(link):
        os.unlink(link)
    os.symlink(os.path.join(os.path.dirname(home), "bin", "herdr"), link)
    subprocess.run(["defaults", "delete", f"herdr.shell.{os.environ['SHELL_LAB']}"], capture_output=True)
    old = jherdr("workspace", "list")["workspaces"]
    ws = jherdr("workspace", "create", "--label", "factory-space", "--cwd", "/tmp", "--no-focus")
    fw = ws["workspace"]["workspace_id"]
    for w in old:
        herdr("workspace", "close", w["workspace_id"])

    def tab(label):
        r = jherdr("tab", "create", "--workspace", fw, "--label", label, "--cwd", "/tmp", "--no-focus")
        return r["tab"]["tab_id"], r["root_pane"]["pane_id"]

    def agent(pane, state):
        for _ in range(200):
            if "%" in herdr("pane", "read", pane, "--source", "visible"):
                break
            time.sleep(0.05)
        herdr("pane", "report-agent", pane, "--source", "spike", "--agent", "claude", "--state", state)

    orch, orch_pane = ws["tab"]["tab_id"], ws["root_pane"]["pane_id"]
    herdr("tab", "rename", orch, "rails orchestrator")
    stuck, stuck_pane = tab("stuck build")        # blocked AND parked: must not lead Needs you
    scope, scope_pane = tab("scope page")          # scoping, live
    old_lane, old_pane = tab("old research")       # parked, idle
    build, build_pane = tab("live build")          # implementing, live
    agent(orch_pane, "working")
    agent(stuck_pane, "blocked")
    agent(scope_pane, "working")
    agent(old_pane, "idle")
    agent(build_pane, "working")

    def lane(t, name, section, kind="lane"):
        return {"tab": t, "name": name, "label": name, "kind": kind, "goal": None, "goal_area": "factory infra",
                "section": section, "section_source": "project", "scope_url": None, "review_url": None, "mode": None}

    lanes = {"version": 1, "generated_at": "2026-10-02T00:00:00Z", "lanes": [
        lane(orch, "rails orchestrator", "orchestrator", "orchestrator"),
        lane(stuck, "stuck build", "implementing"),
        lane(scope, "scope page", "scoping"),
        lane(old_lane, "old research", "implementing"),
        lane(build, "live build", "implementing"),
    ]}
    areas = {"version": 1,
             "areas": [{"id": "factory", "name": "factory", "color": "#5AA9FF"},
                       {"id": "unsorted", "name": "unsorted", "color": "#999999"}],
             "tabs": {orch: {"area": "factory", "role": "top"}},
             "spaces": {fw: "factory"}, "goal_area": {"factory infra": "factory"}, "goal": {}}
    modes = {"version": 1, "tabs": {
        stuck: {"mode": "parked", "at": "2026-10-02T20:25:04.785Z", "by": "p6",
                "note": "Parked 2026-10-02 16:30 ET: factory paused for the postmortem; resume after Alex approves"},
        old_lane: {"mode": "parked", "at": "2026-09-30T14:00:00.000Z", "by": "alex", "note": "come back after the raise"},
    }}
    for name, doc in (("lanes.json", lanes), ("areas.json", areas), ("modes.json", modes)):
        with open(os.path.join(FIX, name), "w") as f:
            json.dump(doc, f)
    kinds = os.path.join(FIX, "workflows", "kinds")
    for f in os.listdir(kinds):
        os.unlink(os.path.join(kinds, f))
    os.environ.update({
        "HERDR_LANES_PATH": os.path.join(FIX, "lanes.json"),
        "HERDR_AREAS_PATH": os.path.join(FIX, "areas.json"),
        "CONTROL_MODES": os.path.join(FIX, "modes.json"),
        "CONTROL_WORKFLOWS": os.path.join(FIX, "workflows"),
        "HERDR_KIND_BIN": "/usr/bin/true",
        # The lab app's HOME is the lab home; point it at the installed lane tool.
        "HERDR_LANE_BIN": os.path.expanduser("~/.local/bin/herdr-lane"),
    })

    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start: {S.app('start').strip()}")
    s = wait_state(lambda s: any(l["kind"] == "parked" for l in s.get("sidebar_lines", [])), 40)
    check("app came up in Areas with a Parked group", s is not None and any(l["kind"] == "parked" for l in s["sidebar_lines"]))
    if s is None:
        return finish()
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "frame", "w": 1280, "h": 820})
    time.sleep(0.5)

    s = S.state()
    names = live(s)
    say(f"  All: {names}")
    check("All leaves both parked tabs out of their area",
          "stuck build" not in names and "old research" not in names and {"scope page", "live build"} <= set(names), f"{names}")
    grp = next(l for l in s["sidebar_lines"] if l["kind"] == "parked")
    check("one Parked group at the foot, shut, with its count",
          s["sidebar_lines"][-1]["id"] == "parked" and grp["chevron"] is False and grp["trailing"] == "2", grp["text"])
    area = next(l for l in s["sidebar_lines"] if l["kind"] == "area")
    check("the factory area count leaves parked tabs out", area["trailing"] == "3", area["text"])
    s = S.state()
    if not s["shell"]["focus_expanded"]:
        s = click_until("focus", None, lambda s: s["shell"]["focus_expanded"] is True)
    focus = focus_titles(s)
    check("Needs you / Focus skips the blocked tab once it is parked", focus == ["scope page"], f"{focus}")
    click_until("focus", None, lambda s: s["shell"]["focus_expanded"] is False)

    s = click_until("chip", "needs", lambda s: s["shell"]["chip"] == "needs")
    check("Needs you chip leaves parked tabs out", live(s) == ["scope page"], f"{live(s)}")

    s = click_until("chip", "parked", lambda s: s["shell"]["chip"] == "parked")
    rows = parked_rows(s)
    check("Parked chip lists only parked rows, newest park first",
          [r["title"] for r in rows] == ["stuck build", "old research"] and live(s) == [], f"{[r['title'] for r in rows]}")
    check("each parked row shows its date and note",
          rows and (rows[0]["park_note"] or "").endswith(" · factory paused for the postmortem; resume after Alex approves")
          and (rows[1]["park_note"] or "").startswith("Sep 30 · come back after the raise"),
          f"{[r['park_note'] for r in rows]}")
    S.cmd({"cmd": "docs", "open": False})
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        time.sleep(0.6)
        shot(f"P26-parked-chip-{mode}.png")

    click_until("chip", "all", lambda s: s["shell"]["chip"] == "all")
    s = click_until("parked", None, lambda s: any(l["parked"] for l in s["sidebar_lines"]))
    check("clicking the group header opens it", len(parked_rows(s)) == 2)
    shot("P26-all-open.png")

    s = click_until("resume", "old research", lambda s: "old research" in live(s))
    doc = modes_doc()
    check("Resume runs herdr-lane unpark: the mode leaves the modes file",
          doc["tabs"].get(old_lane, {}).get("mode") is None, json.dumps(doc["tabs"].get(old_lane)))
    check("the resumed row is back in its area and the group count drops",
          s is not None and "old research" in live(s)
          and next(l for l in s["sidebar_lines"] if l["kind"] == "parked")["trailing"] == "1", f"{live(s) if s else None}")

    S.cmd({"cmd": "park", "tab": build, "note": "waiting on the PC"})
    s = wait_state(lambda s: "live build" not in live(s), 20)
    doc = modes_doc()
    entry = doc["tabs"].get(build, {})
    check("Park runs herdr-lane park with the note and by=herdr-shell",
          entry.get("mode") == "parked" and entry.get("note") == "waiting on the PC" and entry.get("by") == "herdr-shell",
          json.dumps(entry))
    check("the parked row leaves its area at once", s is not None and "live build" not in live(s)
          and any(l["title"] == "live build" for l in parked_rows(s)))

    S.cmd({"cmd": "appearance", "mode": "dark"})
    finish()


def finish():
    S.app("stop")
    time.sleep(0.4)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(S.OUT), exist_ok=True)
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
