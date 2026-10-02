#!/usr/bin/env python3
"""P11 check: detail panel beside the sidebar.

  SHELL_LAB=shellspike-d HERDR_SHELL_APP=<binary> python3 scripts/check_p11.py --out checks/P11.txt

Lab: the seeded layout, plus tokens on the orchestrator (`inbox=3`, the sidebar's numeric
count; `inbox_items`, the panel's text items; `routed`), a `phase` token on one workflow, and a
second space with its own orchestrator, lane and blocked workflow. Then, in the app:
  - every open/switch/close/"open full tab" below is a real mouse click (NSEvent down+up through
    window.sendEvent on the row's or button's reported frame), not a direct call;
  - clicking the orchestrator row shows its inbox, routed items and every workflow under it by
    lane, with host badges and phases, and none of the other space's lanes, workflows or asks;
  - the sidebar count `inbox 3` survives, and the panel does not show "3" as an item;
  - opening a lane row switches the panel to that lane's workflows; a second toggle on
    the same row closes it; the selected tab never changes;
  - THE CHECK from DECISION.md: with the panel open, typed text still reaches the focused
    pane (herdr pane read), and Esc closes the panel without sending ESC to the pane.
    The pane runs `cat -v`, so a stray ESC would print `^[`. With no panel open, Esc does
    reach the pane (`^[` appears): the app claims Esc only while the panel is open;
  - the panel sits between the sidebar and the pane host and never holds keyboard focus.
"""
import json
import os
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-d"
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402  (helpers: lab, app, cmd, state, key, type_, wait_read, pane_read)

lines, failures = [], []


def say(s=""):
    print(s)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def herdr(*a):
    return S.lab("herdr", *a)


def detail(action, **kw):
    """`close` is Esc's own action; every open, switch and 'full' below goes through click()."""
    S.cmd({"cmd": "detail", "action": action, **kw})
    time.sleep(0.25)


def click(label=None, target="row"):
    """A real mouse click on a sidebar row (by title) or the panel's 'open full tab' button."""
    S.cmd({"cmd": "click", "target": target, **({"label": label} if label else {})})
    time.sleep(0.4)


def ready(pane):
    for _ in range(200):
        if "%" in herdr("pane", "read", pane, "--source", "visible"):
            return
        time.sleep(0.05)


def seed_other_space():
    """A second space with an orchestrator, a lane and a blocked workflow: none may show in the first space's panel."""
    r = json.loads(herdr("workspace", "create", "--label", "other-space", "--cwd", "/tmp", "--no-focus"))["result"]
    wid, orch = r["workspace"]["workspace_id"], r["root_pane"]["pane_id"]
    herdr("tab", "rename", r["tab"]["tab_id"], "other orchestrator")

    def tab(label):
        return json.loads(herdr("tab", "create", "--workspace", wid, "--label", label, "--cwd", "/tmp", "--no-focus"))["result"]["root_pane"]["pane_id"]

    lane, wf = tab("other lane"), tab("wf other-secret")
    for p in (orch, lane, wf):
        ready(p)
    for pane, agent, st, toks in ((orch, "claude", "working", {"kind": "orchestrator", "inbox_items": "OTHER-SPACE ITEM@x"}),
                                  (lane, "claude", "blocked", {"kind": "lane"}),
                                  (wf, "codex", "blocked", {"kind": "workflow", "host": "PC", "phase": "other-phase"})):
        herdr("pane", "report-agent", pane, "--source", "spike", "--agent", agent, "--state", st)
        args = ["pane", "report-metadata", pane, "--source", "spike"]
        for k, v in toks.items():
            args += ["--token", f"{k}={v}"]
        herdr(*args)
    herdr("agent", "owner", "set", wf, lane)


def wait_state(pred, timeout=5.0):
    t0 = time.time()
    s = S.state()
    while not pred(s) and time.time() - t0 < timeout:
        time.sleep(0.1)
        s = S.state()
    return s


def main():
    say(f"HerdrShell P11 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    tabs = {t["label"]: t["tab_id"] for t in snap["tabs"]}
    pane_of = {t["label"]: next(p["pane_id"] for p in snap["panes"] if p["tab_id"] == t["tab_id"]) for t in snap["tabs"]}
    lay = next(l for l in snap["layouts"] if l["tab_id"] == tabs["shell spike"])
    p1, p2 = [p["pane_id"] for p in sorted(lay["panes"], key=lambda p: p["rect"]["x"])]
    herdr("pane", "report-metadata", pane_of["rails orchestrator"], "--source", "spike",
          "--token", "inbox=3",
          "--token", "inbox_items=Outreach drafts drop the role title@plugin|Sidebar header height jumps@VPD",
          "--token", "routed=recruiter drafts bug@10:52|workspace embed header@10:48")
    herdr("pane", "report-metadata", pane_of["wf recruiter-2320"], "--source", "spike", "--token", "host=PC", "--token", "phase=impl 3/5")
    seed_other_space()

    subprocess.run(["defaults", "delete", f"herdr.shell.{os.environ['SHELL_LAB']}"], capture_output=True)
    open(os.path.join(S.LAB, "app.log"), "w").close()   # so the log evidence below is from this run
    say(f"app start: {S.app('start').strip()}")
    time.sleep(1.5)
    s = wait_state(lambda s: s["focused_pane"] == p1 and s["selected_tab"] == tabs["shell spike"]
                   and any(r["label"] == "recruiter" and r["children"] for r in s["sidebar"]["lanes"]), 15)
    check("app up, 'shell spike' selected, pane 1 focused", s["focused_pane"] == p1 and s["selected_tab"] == tabs["shell spike"],
          f"tab={s['selected_tab']} pane={s['focused_pane']}")
    d = s["detail"]
    check("panel closed at start and takes no space", d["open"] is False and d["panel_hidden"] is True)
    host_x0 = float(s["host_frame"].strip("{}").replace("}", "").replace("{", "").split(",")[0])
    tab0, focus0 = s["selected_tab"], s["focused_pane"]

    # Warm-up: a real click opens the panel and a second one closes it (the window is key for these).
    click("rails orchestrator")
    check("a real click on the orchestrator row opens the panel", S.state()["detail"]["open"] is True)
    click("rails orchestrator")
    check("a second real click closes it", S.state()["detail"]["open"] is False)
    check("clicks ran without making the window key", "key=false" in open(os.path.join(S.LAB, "app.log")).read().split("hook: click")[-1])

    # Orchestrator row: inbox, routed, workflows under it by lane.
    click("rails orchestrator")
    s = S.state()
    d = s["detail"]
    check("orchestrator click opens the panel for that row", d["open"] and d.get("title") == "rails orchestrator" and d.get("kind") == "orchestrator",
          f"row={d.get('title')}")
    inbox = [(i["text"], i["source"]) for i in d.get("inbox", [])]
    check("inbox shows the tokened items and the blocked workflow that wants you",
          inbox[:2] == [("Outreach drafts drop the role title", "plugin"), ("Sidebar header height jumps", "VPD")]
          and ("wf embed wave-a wants you", "blocked") in inbox, f"inbox={inbox}")
    hdr = next((l for l in s["sidebar_lines"] if l["id"] == "hdr:orchestrator"), {})
    check("inbox is not shown as an item, and the sidebar keeps its numeric count",
          all(i[0] != "3" for i in inbox) and hdr.get("trailing") == "inbox 3", f"header trailing={hdr.get('trailing')!r}")
    routed = [(i["text"], i["source"]) for i in d.get("routed", [])]
    check("routed items shown for the orchestrator", routed == [("recruiter drafts bug", "10:52"), ("workspace embed header", "10:48")], f"routed={routed}")
    groups = {g["lane"]: [(w["label"], w["phase"], w["host"]) for w in g["workflows"]] for g in d.get("groups", [])}
    check("workflows under the orchestrator: its own folded one, then each lane's, with phases and host badges",
          groups.get(None) == [("wf embed wave-a", "blocked", "PC")]
          and groups.get("recruiter") == [("wf recruiter-2320", "impl 3/5", "PC")], f"groups={groups}")
    everything = json.dumps(d)
    check("the other space's lane, workflow, ask and inbox never show in this space's panel",
          not any(w in everything for w in ("other lane", "wf other-secret", "other-phase", "OTHER-SPACE")), "checked panel state for other-space rows")
    check("panel sits between the sidebar and the pane host and never holds focus",
          d["panel_frame"][0] > 0 and d["panel_frame"][2] == 340 and d["panel_frame"][0] + d["panel_frame"][2] < 1400
          and d["panel_first_responder"] is False, f"panel_frame={d['panel_frame']}")
    check("opening the panel leaves the selected tab and the focused pane alone",
          s["selected_tab"] == tab0 and s["focused_pane"] == focus0, f"tab={s['selected_tab']} pane={s['focused_pane']}")
    host_x1 = float(s["host_frame"].strip("{}").replace("}", "").replace("{", "").split(",")[0])
    check("pane host moved right by the panel width, sidebar did not move",
          abs(host_x1 - (host_x0 + 341)) < 1.5, f"host x {host_x0} -> {host_x1}")

    # THE CHECK: typing reaches the pane with the panel open.
    S.type_("echo p11-typed-ok")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "p11-typed-ok" for l in x.splitlines()))
    check("with the panel open, typed text still reaches the focused pane (herdr pane read)", dt is not None,
          f"output line seen {dt:.3f}s after the last key" if dt is not None else txt[-300:])
    s = S.state()
    check("panel still open after typing, pane 1 still first responder", s["detail"]["open"] and s["focused_pane"] == p1)

    # THE CHECK: Esc closes the panel and no ESC reaches the pane.
    S.type_("cat -v")
    S.key("return")
    S.wait_read(p1, lambda x: any(l.strip() == "cat -v" for l in x.splitlines()))
    time.sleep(0.3)
    S.type_("a")
    S.key("escape")
    s = wait_state(lambda s: not s["detail"]["open"], 3)
    check("Esc closed the panel", s["detail"]["open"] is False and s["detail"]["panel_hidden"] is True)
    host_x2 = float(s["host_frame"].strip("{}").replace("}", "").replace("{", "").split(",")[0])
    check("pane host is back at full width after Esc", abs(host_x2 - host_x0) < 1.5, f"host x {host_x2}")
    S.type_("b")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "ab" for l in x.splitlines()))
    check("Esc did not send ESC into the pane: cat -v echoed 'ab', not 'a^[b'", dt is not None and "a^[b" not in txt,
          f"tail={[l for l in txt.splitlines() if l.strip()][-3:]}")
    check("the log shows Esc fired close_detail", "action close_detail chord=escape" in open(os.path.join(S.LAB, "app.log")).read())

    # With no panel open, Esc is the pane's.
    S.key("escape")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: "^[" in x)
    check("with no panel open Esc reaches the pane (cat -v echoed ^[)", dt is not None, f"tail={[l for l in txt.splitlines() if l.strip()][-3:]}")
    S.key("c", ["ctrl"])
    time.sleep(0.2)

    # Lane row, switching, second click closes; tab never changes.
    click("recruiter")
    s = S.state()
    d = s["detail"]
    lane_groups = {g["lane"]: [(w["label"], w["phase"], w["host"]) for w in g["workflows"]] for g in d.get("groups", [])}
    check("lane click opens that lane's panel with its workflows, no routed section",
          d["open"] and d.get("title") == "recruiter" and d.get("kind") == "lane" and lane_groups == {None: [("wf recruiter-2320", "impl 3/5", "PC")]}
          and d.get("routed") == [], f"row={d.get('title')} groups={lane_groups}")
    click("rails orchestrator")
    s = S.state()
    check("clicking another row switches the panel to it", s["detail"]["open"] and s["detail"].get("title") == "rails orchestrator")
    click("rails orchestrator")
    s = S.state()
    check("a second click on the open row closes the panel", s["detail"]["open"] is False)
    check("selected tab unchanged through all of it", s["selected_tab"] == tab0 and s["focused_pane"] == focus0)
    click("recruiter")
    click(target="open_full")
    s = S.state()
    check("'open full tab' button click selects the row's tab and closes the panel",
          s["selected_tab"] == tabs["recruiter"] and s["detail"]["open"] is False, f"tab={s['selected_tab']}")

    # Evidence image: panel open over the shell-spike tab.
    detail("close")
    S.cmd({"cmd": "select", "tab": tabs["shell spike"]})
    time.sleep(0.4)
    click("rails orchestrator")
    time.sleep(0.5)
    shot = os.path.splitext(S.OUT)[0] + ".png"
    if os.path.exists(shot):
        os.unlink(shot)
    S.cmd({"cmd": "shot", "out": shot})
    for _ in range(100):
        if os.path.exists(shot) and os.path.getsize(shot) > 0:
            break
        time.sleep(0.1)
    pshot = os.path.splitext(S.OUT)[0] + "-panel.png"
    if os.path.exists(pshot):
        os.unlink(pshot)
    S.cmd({"cmd": "panel_shot", "out": pshot})
    for _ in range(100):
        if os.path.exists(pshot) and os.path.getsize(pshot) > 0:
            break
        time.sleep(0.1)
    say(f"panel image (SwiftUI ImageRenderer of the open panel): {os.path.basename(pshot)} exists={os.path.exists(pshot)}")
    say(f"screenshot (in-app capture, panel open on the orchestrator): {os.path.basename(shot)} exists={os.path.exists(shot)}")

    S.check_front(check)
    S.app("stop")
    time.sleep(0.5)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
