#!/usr/bin/env python3
"""P15 check: sidebar groups by area; stage is a filter; Focus; setup restores.

  SHELL_LAB=shellspike-areas python3 scripts/check_p15.py [--out checks/P15.txt]

Fixtures (HERDR_LANES_PATH / HERDR_AREAS_PATH) cover two spaces, an orchestrator,
a desk, two jobs, projects in scoping / implementing / reviewing / closed, one
blocked lane, and one tab absent from lanes.json. Asserts from the state dump
and real clicks. Relaunches once and checks the saved setup came back.
"""
import json
import os
import shutil
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-areas"
# The pane-attach spike binary this lab copies in. The shared cache path is not on
# this machine; an existing lab copy is the same binary.
os.environ.setdefault(
    "HERDR_SHELL_BIN",
    os.path.expanduser("~/.cache/herdr-build/shellspike-q/bin/herdr"),
)
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
    S.OUT = os.path.join(D0, "checks", "P15.txt")


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


def texts(s):
    return [l["text"] for l in s["sidebar_lines"]]


def rows(s):
    return [l for l in s["sidebar_lines"] if l["tab"] and not str(l["id"]).startswith("focus:")]


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


def click(target, label=None, mods=None):
    body = {"cmd": "click", "target": target}
    if label is not None:
        body["label"] = label
    if mods:
        body["mods"] = mods
    S.cmd(body)
    time.sleep(0.45)


def titles(s, area=None, kind=None):
    out = []
    for l in s["sidebar_lines"]:
        if l["kind"] in ("header", "area", "focus", "note"):
            continue
        if str(l["id"]).startswith("focus:"):
            continue
        if area is not None and l.get("area") != area:
            continue
        if kind is not None and l["kind"] != kind:
            continue
        out.append(l["title"])
    return out


def subheads(s):
    return [l["title"] for l in s["sidebar_lines"] if l["kind"] == "header"]


def main():
    say(f"HerdrShell P15 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    # Drop a previous run's saved setup so Areas comes up as the default.
    prefs = os.path.join(LABDIR, "h", "Library", "Preferences")
    if os.path.isdir(prefs):
        shutil.rmtree(prefs, ignore_errors=True)
    S.lab("up")
    # Saved setup is a per-lab UserDefaults suite in the real home, not the lab home
    # (cfprefsd ignores HOME). Drop it so Areas/All is the default for this run.
    subprocess.run(["defaults", "delete", "herdr.shell.shellspike-areas"], capture_output=True)
    # Two spaces. The lab's stock workspace is closed after these exist.
    old = jherdr("workspace", "list")["workspaces"]
    factory = jherdr("workspace", "create", "--label", "factory-space", "--cwd", "/tmp", "--no-focus")
    raise_ws = jherdr("workspace", "create", "--label", "raise-space", "--cwd", "/tmp", "--no-focus")
    fw, rw = factory["workspace"]["workspace_id"], raise_ws["workspace"]["workspace_id"]
    for w in old:
        herdr("workspace", "close", w["workspace_id"])

    def tab(ws, label):
        r = jherdr("tab", "create", "--workspace", ws, "--label", label, "--cwd", "/tmp", "--no-focus")
        return r["tab"]["tab_id"], r["root_pane"]["pane_id"]

    def ready(pane):
        for _ in range(200):
            if "%" in herdr("pane", "read", pane, "--source", "visible"):
                return
            time.sleep(0.05)

    def agent(pane, name, state, **tok):
        ready(pane)
        herdr("pane", "report-agent", pane, "--source", "spike", "--agent", name, "--state", state)
        if tok:
            args = ["pane", "report-metadata", pane, "--source", "spike"]
            for k, v in tok.items():
                args += ["--token", f"{k}={v}"]
            herdr(*args)

    # factory-space keeps its root tab: rename it to the orchestrator.
    orch = factory["tab"]["tab_id"]
    orch_pane = factory["root_pane"]["pane_id"]
    herdr("tab", "rename", orch, "rails orchestrator")
    blocked, blocked_pane = tab(fw, "blocked build")
    scope_f, scope_f_pane = tab(fw, "scope factory")
    stray, stray_pane = tab(fw, "stray tab")
    # raise-space root becomes the scoping project.
    scope_r = raise_ws["tab"]["tab_id"]
    scope_r_pane = raise_ws["root_pane"]["pane_id"]
    herdr("tab", "rename", scope_r, "raise brief raw")
    review, review_pane = tab(rw, "review packet")
    build, build_pane = tab(rw, "raise build")
    wf, wf_pane = tab(rw, "wf raise-1")
    closed, closed_pane = tab(rw, "old raise")
    desk, desk_pane = tab(rw, "recruiting desk raw")
    job1, job1_pane = tab(rw, "job outreach raw")
    job2, job2_pane = tab(rw, "job followups")

    agent(orch_pane, "claude", "working", kind="orchestrator")
    agent(blocked_pane, "claude", "blocked", kind="lane")
    agent(scope_f_pane, "claude", "working", kind="lane")
    agent(stray_pane, "claude", "idle", kind="lane")
    agent(scope_r_pane, "claude", "working", kind="lane")
    agent(review_pane, "claude", "working", kind="lane")
    agent(build_pane, "claude", "working", kind="lane")
    agent(wf_pane, "codex", "working", kind="workflow")
    herdr("agent", "owner", "set", wf_pane, build_pane)
    agent(closed_pane, "claude", "idle", kind="lane")
    agent(desk_pane, "claude", "idle", kind="lane")
    agent(job1_pane, "claude", "working", kind="lane")
    agent(job2_pane, "claude", "idle", kind="lane")

    lanes = {
        "version": 1,
        "generated_at": "2026-10-01T00:00:00Z",
        "lanes": [
            {"tab": orch, "name": "orchestrator", "label": "orchestrator", "kind": "orchestrator",
             "goal": None, "goal_area": None, "section": "orchestrator", "section_source": "explicit",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": blocked, "name": "blocked build", "label": "blocked build", "kind": "lane",
             "goal": None, "goal_area": "factory infra", "section": "implementing", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": scope_f, "name": "scope factory", "label": "scope factory", "kind": "lane",
             "goal": None, "goal_area": "factory infra", "section": "scoping", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": scope_r, "name": "[scoping] raise brief", "label": "raise brief raw", "kind": "lane",
             "goal": "raise", "goal_area": None, "section": "scoping", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": review, "name": "review packet", "label": "review packet", "kind": "lane",
             "goal": "raise", "goal_area": None, "section": "reviewing", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": build, "name": "raise build", "label": "raise build", "kind": "lane",
             "goal": "raise", "goal_area": None, "section": "implementing", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": wf, "name": "wf raise-1", "label": "wf raise-1", "kind": "workflow",
             "goal": None, "goal_area": None, "section": None, "section_source": "default",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": closed, "name": "old raise", "label": "old raise", "kind": "lane",
             "goal": "raise", "goal_area": None, "section": "closed", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": desk, "name": "recruiting desk", "label": "recruiting desk raw", "kind": "lane",
             "goal": None, "goal_area": None, "section": "implementing", "section_source": "default",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": job1, "name": "job outreach", "label": "job outreach raw", "kind": "lane",
             "goal": None, "goal_area": None, "section": "implementing", "section_source": "default",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": job2, "name": "job followups", "label": "job followups", "kind": "lane",
             "goal": None, "goal_area": None, "section": "implementing", "section_source": "default",
             "scope_url": None, "review_url": None, "mode": None},
        ],
    }
    areas = {
        "version": 1,
        "areas": [
            {"id": "factory", "name": "factory", "color": "#5AA9FF"},
            {"id": "raise", "name": "raise", "color": "#B5476B"},
            {"id": "recruiter", "name": "recruiter", "color": "#4F5BD5"},
            {"id": "empty", "name": "empty", "color": "#888888"},
            {"id": "unsorted", "name": "unsorted", "color": "#999999"},
        ],
        "tabs": {
            orch: {"area": "factory", "role": "top", "name": "rails orchestrator"},
            desk: {"area": "recruiter", "role": "desk", "name": "recruiting desk"},
            job1: {"area": "recruiter", "role": "job", "name": "job outreach"},
            job2: {"role": "job"},
        },
        "spaces": {fw: "factory", rw: "raise"},
        "goal_area": {"factory infra": "factory"},
        "goal": {"raise": "raise"},
    }
    # job2 has a role but no area: the raise space maps the whole workspace to raise,
    # which would put the jobs in raise. Give job2 an explicit area via the tab entry's
    # missing area falling through... space rw is raise, so job2 would be raise.
    # Pin jobs to recruiter with an explicit area so Use is its own area.
    areas["tabs"][job2] = {"area": "recruiter", "role": "job"}
    with open(os.path.join(FIX, "lanes.json"), "w") as f:
        json.dump(lanes, f)
    with open(os.path.join(FIX, "areas.json"), "w") as f:
        json.dump(areas, f)
    os.environ["HERDR_LANES_PATH"] = os.path.join(FIX, "lanes.json")
    os.environ["HERDR_AREAS_PATH"] = os.path.join(FIX, "areas.json")

    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start: {S.app('start').strip()}")
    # P33 (Alex, 2026-10-03): Spaces is the default; this scenario then works in Areas.
    s0 = wait_state(lambda s: s.get("shell", {}).get("mode") in ("spaces", "areas"), 40)
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "click", "target": "mode", "label": "areas"})
    s = wait_state(lambda s: s.get("shell", {}).get("mode") == "areas" and any(l["kind"] == "area" for l in s["sidebar_lines"]), 40)
    check("app state readable; Spaces is the default, Areas one click away",
          s0 is not None and s0.get("shell", {}).get("mode") == "spaces" and s is not None,
          f"first mode={None if s0 is None else s0.get('shell', {}).get('mode')}")
    if s is None:
        return finish()

    time.sleep(0.4)
    S.cmd({"cmd": "frame", "w": 1440, "h": 900})
    time.sleep(0.4)
    s = S.state()

    area_ids = [l["area"] for l in s["sidebar_lines"] if l["kind"] == "area"]
    check("areas draw in areas[] order and an empty area is omitted",
          area_ids == ["factory", "raise", "recruiter"] and "empty" not in area_ids, f"areas={area_ids}")
    heads = subheads(s)
    check("sub-headers only when that group has rows",
          heads == ["ORCHESTRATOR", "PROJECTS", "PROJECTS", "USE"], f"headers={heads}")
    fac = titles(s, "factory")
    rai = titles(s, "raise")
    rec = titles(s, "recruiter")
    check("factory: orchestrator, then projects (blocked, scoping, the tab lanes.json lacks)",
          fac[:4] == ["rails orchestrator", "blocked build", "scope factory", "stray tab"] or fac == ["rails orchestrator", "blocked build", "scope factory", "stray tab"],
          f"{fac}")
    check("raise: scoping name had [scoping] stripped, closed row is last and dimmed",
          rai[:3] == ["raise brief", "review packet", "raise build"] and rai[-1:] == ["old raise"],
          f"{rai}")
    closed_row = next((l for l in s["sidebar_lines"] if l["title"] == "old raise"), None)
    check("closed row is dimmed and hidden from every chip but All (asserted per chip below)",
          closed_row is not None and closed_row["dim"] is True and closed_row["stage"] == "closed")
    check("recruiter Use group is desk then jobs",
          rec == ["recruiting desk", "job outreach", "job followups"], f"{rec}")
    stray_row = next(l for l in s["sidebar_lines"] if l["title"] == "stray tab")
    check("tab absent from lanes.json still shows, stage none, area by goal/space rules (factory)",
          stray_row["stage"] is None and stray_row["area"] == "factory" and stray_row["role"] == "project")
    check("every drawn line carries kind, area, role, stage, text, selected, dim",
          all(k in l for l in s["sidebar_lines"] for k in ("kind", "area", "role", "stage", "text", "selected", "dim")))

    # Workflow stays under its owner. Fold it shut again before the chip lists.
    S.cmd({"cmd": "sidebar_fold", "id": f"tab:{build}", "open": True})
    s = wait_state(lambda s: any(l["title"] == "wf raise-1" for l in s["sidebar_lines"]))
    wf_line = next((l for l in s["sidebar_lines"] if l["title"] == "wf raise-1"), None)
    check("a workflow stays folded under its owner, not as its own area row",
          wf_line is not None and wf_line["kind"] == "workflow" and wf_line["depth"] == 2, f"{wf_line}")
    S.cmd({"cmd": "sidebar_fold", "id": f"tab:{build}", "open": False})
    wait_state(lambda s: not any(l["title"] == "wf raise-1" for l in s["sidebar_lines"]))

    def chip_titles(chip):
        click("chip", chip)
        st = wait_state(lambda s: s["shell"]["chip"] == chip)
        names = [l["title"] for l in st["sidebar_lines"] if l.get("tab") and not str(l["id"]).startswith("focus:")]
        say(f"  chip {chip}: {names}")
        return names

    say("rows per chip:")
    click("chip", "all")
    all_names = chip_titles("all")
    check("All shows every top-level row, closed included",
          "old raise" in all_names and "rails orchestrator" in all_names and "stray tab" in all_names and "job followups" in all_names,
          f"{all_names}")
    needs = chip_titles("needs")
    check("Needs you is reviewing, scoping, or blocked",
          needs == ["blocked build", "scope factory", "raise brief", "review packet"] or set(needs) == {"blocked build", "scope factory", "raise brief", "review packet"},
          f"{needs}")
    scoping_rows = chip_titles("scoping")
    check("Scoping chip is the scoping stage only", set(scoping_rows) == {"scope factory", "raise brief"}, f"{scoping_rows}")
    building_rows = chip_titles("building")
    check("Building chip is implementing (including the desk and jobs, and the blocked lane)",
          set(building_rows) == {"blocked build", "raise build", "recruiting desk", "job outreach", "job followups"}, f"{building_rows}")
    review_rows = chip_titles("review")
    check("Review chip is the reviewing stage only", review_rows == ["review packet"], f"{review_rows}")
    use_rows = chip_titles("use")
    check("Use chip is desk then jobs", use_rows == ["recruiting desk", "job outreach", "job followups"], f"{use_rows}")
    click("chip", "all")

    # Focus ranking and stepping. Start from a row that is not in the queue.
    click("row", "recruiting desk")
    time.sleep(0.3)
    click("focus")
    s = wait_state(lambda s: s["shell"]["focus_expanded"] is True)
    focus_rows = [l["title"] for l in s["sidebar_lines"] if str(l["id"]).startswith("focus:")]
    check("Focus ranks blocked, then reviewing, then scoping (area order breaks scoping ties)",
          focus_rows == ["blocked build", "review packet", "scope factory", "raise brief"], f"{focus_rows}")
    want_focus = ["blocked build", "review packet", "scope factory"]
    herdr_focus = []
    for label in want_focus:
        S.key("]", ["cmd"])
        t0 = time.time()
        got = None
        while time.time() - t0 < 8:
            snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
            got = next((t["label"] for t in snap["tabs"] if t.get("focused")), None)
            if got == label:
                break
            time.sleep(0.25)
        herdr_focus.append(got)
    check("cmd+] three times selects blocked, then reviewing, then the first scoping tab",
          herdr_focus == ["blocked build", "review packet", "scope factory"], f"{herdr_focus}")
    s = S.state()
    focus_line = next(l for l in s["sidebar_lines"] if l["id"] == "focus")
    check("Focus row shows the step as '3 of N'", "3 of " in focus_line["text"], focus_line["text"])
    S.key("[", ["cmd"])
    t0 = time.time()
    back = None
    while time.time() - t0 < 8:
        snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
        back = next((t["label"] for t in snap["tabs"] if t.get("focused")), None)
        if back == "review packet":
            break
        time.sleep(0.25)
    check("cmd+[ steps back one in that ranking", back == "review packet", f"focused={back}")

    # Fold and area-only, real clicks.
    click("chip", "all")
    click("area", "raise")
    s = wait_state(lambda s: "raise" in s["shell"]["folded_areas"])
    check("clicking an area header folds it",
          s is not None and "raise brief" not in titles(s) and "raise" in s["shell"]["folded_areas"])
    click("area", "raise")
    s = wait_state(lambda s: "raise" not in s["shell"]["folded_areas"])
    click("area", "factory", ["opt"])
    s = wait_state(lambda s: s["shell"]["area_only"] == "factory", 8)
    only = next((l["text"] for l in [] if False), "")
    # The chip is not a sidebar line; the shell field and the visible areas are.
    areas_now = [l["title"] for l in s["sidebar_lines"] if l["kind"] == "area"] if s else []
    check("option-click shows only that area",
          s is not None and s["shell"]["area_only"] == "factory" and areas_now == ["factory"], f"areas={areas_now} only={s['shell']['area_only'] if s else None}")
    click("only")
    s = wait_state(lambda s: s["shell"]["area_only"] is None)
    check("the only-chip clears the area filter", s is not None and s["shell"]["area_only"] is None)

    # Spaces mode keeps the P10 line shape. The open space is the selected tab's,
    # so stand on the factory orchestrator where ORCHESTRATOR and LANES both exist.
    click("row", "rails orchestrator")
    time.sleep(0.3)
    S.key("a", ["cmd", "shift"])
    s = wait_state(lambda s: s["shell"]["mode"] == "spaces")
    kinds = [l["kind"] for l in s["sidebar_lines"]] if s else []
    check("cmd+shift+a switches to Spaces and the lines keep the P10 shape (SPACES, space, orchestrator, lanes; no area groups)",
          s is not None and texts(s)[:1] == ["SPACES"] and "area" not in kinds and "focus" not in kinds
          and any(t.startswith("ORCHESTRATOR") for t in texts(s)) and any(l["kind"] == "space" for l in s["sidebar_lines"])
          and any(l["kind"] == "lane" for l in s["sidebar_lines"]),
          f"first={texts(s)[:6] if s else None}")
    S.key("a", ["cmd", "shift"])
    s = wait_state(lambda s: s["shell"]["mode"] == "areas")

    # Screenshots.
    click("chip", "all")
    S.cmd({"cmd": "docs", "open": False})
    time.sleep(0.3)
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        time.sleep(0.6)
        png = os.path.join(CHK, f"P15-{mode}.png")
        if os.path.exists(png):
            os.unlink(png)
        S.cmd({"cmd": "shot", "out": png})
        for _ in range(80):
            if os.path.exists(png) and os.path.getsize(png) > 1000:
                break
            time.sleep(0.1)
        check(f"screenshot {mode}: checks/P15-{mode}.png", os.path.exists(png) and os.path.getsize(png) > 1000, png)
    try:
        from PIL import Image
        px = {}
        for mode in ("light", "dark"):
            im = Image.open(os.path.join(CHK, f"P15-{mode}.png")).convert("RGB")
            px[mode] = (im.size, im.getpixel((80, 200)))
        check("light and dark screenshots differ", px["light"][1] != px["dark"][1], f"{px}")
    except Exception as e:  # noqa: BLE001
        check("screenshot pixels readable", False, repr(e))

    # Persist, then relaunch once. Fold factory while every area is on screen.
    click("chip", "all")
    click("only")
    s = wait_state(lambda s: s["shell"]["chip"] == "all" and s["shell"]["area_only"] is None)
    if s and "factory" not in s["shell"]["folded_areas"]:
        click("area", "factory")
    s = wait_state(lambda s: "factory" in s["shell"]["folded_areas"])
    if s and not s["shell"]["focus_expanded"]:
        click("focus")
    click("area", "raise", ["opt"])
    click("chip", "review")
    s = wait_state(lambda s: s["shell"]["chip"] == "review" and s["shell"]["area_only"] == "raise")
    click("row", "review packet")
    time.sleep(0.3)
    S.cmd({"cmd": "docs", "open": True, "width": 510})
    time.sleep(0.3)
    S.key("a", ["cmd", "shift"])
    s = wait_state(lambda s: s["shell"]["mode"] == "spaces" and s["shell"]["doc_open"] is True and abs(s["shell"]["doc_width"] - 510) < 1)
    before = s["shell"] if s else {}
    say(f"saved setup: {json.dumps(before, default=str)}")
    check("setup to restore is spaces / review / only raise / factory folded / focus open / docs 510",
          before.get("mode") == "spaces" and before.get("chip") == "review" and before.get("area_only") == "raise"
          and "factory" in before.get("folded_areas", []) and before.get("focus_expanded") is True
          and before.get("doc_open") is True and abs(before.get("doc_width", 0) - 510) < 1
          and before.get("selected_tab") == review, f"{before}")

    S.app("stop")
    time.sleep(0.8)
    say(f"relaunch: {S.app('start').strip()}")
    s = wait_state(lambda s: s.get("shell", {}).get("mode") == "spaces" and s.get("selected_tab") == review, 40)
    sh = s["shell"] if s else {}
    say(f"restored setup: {json.dumps(sh, default=str)}")
    check("relaunch restores mode", sh.get("mode") == "spaces", f"mode={sh.get('mode')}")
    check("relaunch restores the active chip", sh.get("chip") == "review", f"chip={sh.get('chip')}")
    check("relaunch restores the area-only filter", sh.get("area_only") == "raise", f"only={sh.get('area_only')}")
    check("relaunch restores folded areas", "factory" in sh.get("folded_areas", []), f"folded={sh.get('folded_areas')}")
    check("relaunch restores Focus expanded", sh.get("focus_expanded") is True, f"focus={sh.get('focus_expanded')}")
    check("relaunch restores the selected tab", s is not None and s.get("selected_tab") == review, f"tab={None if s is None else s.get('selected_tab')}")
    check("relaunch restores the doc panel open and its width",
          sh.get("doc_open") is True and abs(sh.get("doc_width", 0) - 510) < 1
          and abs(sh.get("doc_frame_width", 0) - 510) < 2,
          f"open={sh.get('doc_open')} width={sh.get('doc_width')} frame={sh.get('doc_frame_width')}")
    # Back to Areas: the chip, fold and area filter are still in effect.
    S.key("a", ["cmd", "shift"])
    s = wait_state(lambda s: s["shell"]["mode"] == "areas")
    if s:
        names = [l["title"] for l in s["sidebar_lines"] if l.get("tab") and not str(l["id"]).startswith("focus:")]
        areas_back = [l["title"] for l in s["sidebar_lines"] if l["kind"] == "area"]
        check("after restore, Areas still applies the review chip and the raise-only filter",
              names == ["review packet"] and areas_back == ["raise"], f"rows={names} areas={areas_back}")
    finish()


def finish():
    S.check_front(check)
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
