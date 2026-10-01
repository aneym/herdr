#!/usr/bin/env python3
"""P10 check: the sidebar 1:1 with the mock. Writes checks/P10.txt (+ P10-light.png, P10-dark.png).

  SHELL_LAB=shellspike-q python3 scripts/check_p10.py [--out checks/P10.txt]

Seeds a lab that matches the mock's Rails space (~/.claude/pretty-docs/factory-devenv-mock-2026-09-28.html,
"after . herdr 0.9 fork . grouped by kind" and the SPACES frame): three visible spaces (agent-rails,
agent-lb, homebase) and three hidden ones; in agent-rails an orchestrator, seven lanes with folded
workflows on PC, and three background tabs. Then asserts from the app's state dump
(`sidebar_lines`, the same lines the view draws) that the rows, their order and fold state equal the
expected list below, and that the come-forward rules hold:

  - a lane with a blocked or failed workflow opens on its own; a hand-set fold wins over that;
  - a row that asks (an advisor) leaves `background` and becomes a lane again;
  - another space's row shows `● n` when n of its rows want you, and `●` when one is working;
  - hidden spaces stay in one collapsed `hidden n` row until it is opened;
  - the space chord (cmd+shift+n, keymap goto_space_n) switches space, and a plain space shows lanes only.

Screenshots for Opus, one per appearance (in-app capture). Lab session only.

Deviations from the mock, on purpose (see README): a lane with no workflows has no chevron; finished workflows that a lane owns stay
under it (with a check mark) and only ownerless finished ones go to background.
"""
import json
import os
import sys
import time

os.environ.setdefault("SHELL_LAB", "shellspike-q")
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LABDIR = os.path.expanduser(f"~/.cache/herdr-build/{os.environ['SHELL_LAB']}")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
# A private copy of the binary: app.py stops apps by matching the binary path, so this keeps
# the check from stopping (or being stopped by) another piece's app run.
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402  (helpers: lab, app, cmd, state, herdr_json, key)

lines, failures = [], []
CHK = os.path.dirname(S.OUT) if "--out" in sys.argv else os.path.join(D0, "checks")
if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "P10.txt")


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


# --- seeding ---------------------------------------------------------------------------

AGENTS = []          # (pane, agent, state, tokens)
OWNERS = []          # (child pane, owner pane)
PANES = {}           # tab label -> root pane


def new_ws(label, tokens=None):
    r = jherdr("workspace", "create", "--label", label, "--cwd", "/tmp", "--no-focus")
    ws = r["workspace"]["workspace_id"]
    if tokens:
        args = ["workspace", "report-metadata", ws, "--source", "spike"]
        for k, v in tokens.items():
            args += ["--token", f"{k}={v}"]
        herdr(*args)
    return ws, r["root_pane"]["pane_id"], r["tab"]["tab_id"]


def new_tab(ws, label, agent=None, state="working", tokens=None, owner=None):
    pane = jherdr("tab", "create", "--workspace", ws, "--label", label, "--cwd", "/tmp", "--no-focus")["root_pane"]["pane_id"]
    register(pane, label, agent, state, tokens, owner)
    return pane


def register(pane, label, agent, state, tokens, owner):
    PANES[label] = pane
    if agent:
        AGENTS.append((pane, agent, state, tokens or {}))
    if owner:
        OWNERS.append((pane, owner))


def report(pane, agent, state, tokens):
    herdr("pane", "report-agent", pane, "--source", "spike", "--agent", agent, "--state", state)
    if tokens:
        args = ["pane", "report-metadata", pane, "--source", "spike"]
        for k, v in tokens.items():
            args += ["--token", f"{k}={v}"]
        herdr(*args)


def flush_agents():
    # An agent report made while the pane's shell is still starting is dropped; wait for a prompt.
    for pane, *_ in AGENTS:
        for _ in range(200):
            if "%" in herdr("pane", "read", pane, "--source", "visible"):
                break
            time.sleep(0.05)
    for pane, agent, state, tokens in AGENTS:
        report(pane, agent, state, tokens)
    for child, owner in OWNERS:
        herdr("agent", "owner", "set", child, owner)


PC = {"host": "PC"}


def seed():
    old = jherdr("workspace", "list")["workspaces"]
    # agent-rails, in the order the mock lists it.
    ws, orch, orch_tab = new_ws("agent-rails", {"pinned": "true"})
    herdr("tab", "rename", orch_tab, "rails orchestrator")
    register(orch, "rails orchestrator", "claude", "working", {"kind": "orchestrator", "inbox": "3"}, None)
    lane = lambda label, state="working", **tok: new_tab(ws, label, "claude", state, {"kind": "lane", **tok})
    recruiter = lane("recruiter")
    embed = lane("workspace embed", dev_loop="rails-local")
    blocks = lane("agent blocks")
    infra = lane("factory infra")
    proto = lane("proto coach")
    lane("recruiter outreach")
    lane("conversation evals", "idle")
    wf = lambda label, owner, state="working", **tok: new_tab(ws, label, "codex", state, {"kind": "workflow", **PC, **tok}, owner)
    wf("wf recruiter-2320", recruiter)
    wf("wf recruiter-2315", recruiter)
    wf("wf embed wave-a", embed, "blocked")
    wf("wf devloop 1", embed, "idle", phase="done")
    wf("wf blocks fold 5", blocks)
    wf("wf blocks fold 6", blocks)
    for i in (1, 2, 3):
        wf(f"wf infra {i}", infra)
    wf("wf proto 1", proto)
    wf("wf reap", orch, "blocked")
    # Background: two advisors and a finished lane.
    new_tab(ws, "step back", "claude", "idle", {"kind": "advisor"})
    new_tab(ws, "toyo research", "claude", "idle", {"kind": "advisor"})
    new_tab(ws, "recruiting email", "claude", "idle", {"kind": "lane", "phase": "done"})
    # Other spaces.
    _, lb, lb_tab = new_ws("agent-lb", {"pinned": "true"})
    herdr("tab", "rename", lb_tab, "agent-lb work")
    register(lb, "agent-lb work", "claude", "idle", {"kind": "lane"}, None)
    hb, hb_pane, hb_tab = new_ws("homebase", {"pinned": "true"})
    register(hb_pane, "homebase main", "claude", "working", {"kind": "lane"}, None)
    herdr("tab", "rename", hb_tab, "aside hls playback")
    new_tab(hb, "dev server")
    new_tab(hb, "notes")
    for name in ("archive-a", "archive-b", "archive-c"):
        new_ws(name, {"hidden": "true"})
    # Retire the lab's stock workspace so only the mock's spaces remain.
    for w in old:
        herdr("workspace", "close", w["workspace_id"])
    flush_agents()
    return ws


# --- expected rows (mock order) ----------------------------------------------------------

def expected(sel_lane_open=None):
    """Text of every line the Rails space shows at rest. Fold state is the chevron."""
    return [
        "SPACES  ⌘⇧1..9",
        "▾ ◆ agent-rails  factory",
        "ORCHESTRATOR  inbox 3",
        "▾ ● rails orchestrator  1 wf  [Studio]",
        "  ◐ wf reap  [PC]",
        "LANES  7 open",
        "▸ ● recruiter  2 wf",
        "▾ ● workspace embed ⟳  2 wf",
        "  ◐ wf embed wave-a  [PC]",
        "  ✓ wf devloop 1  [PC]",
        "▸ ● agent blocks  2 wf",
        "▸ ● factory infra  3 wf",
        "▸ ● proto coach  1 wf",
        "● recruiter outreach",
        "○ conversation evals  idle",
        "▸ background  3",
        "  step back · toyo research · recruiting email ✓",
        "▸ ◆ agent-lb  1",
        "▸ ◆ homebase  ●",
        "▸ hidden  3",
    ]


def texts(s):
    return [l["text"] for l in s["sidebar_lines"]]


def wait_lines(pred, timeout=8.0):
    t0 = time.time()
    s = None
    while time.time() - t0 < timeout:
        try:
            s = S.state()
        except SystemExit:
            time.sleep(0.1)
            continue
        if pred(s):
            return s
        time.sleep(0.1)
    return s


def diff(want, got):
    out = []
    for i in range(max(len(want), len(got))):
        w = want[i] if i < len(want) else "<none>"
        g = got[i] if i < len(got) else "<none>"
        if w != g:
            out.append(f"line {i}: want {w!r} got {g!r}")
    return "; ".join(out[:4])


def main():
    say(f"HerdrShell P10 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    ws = seed()
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    label = {t["tab_id"]: t["label"] for t in snap["tabs"]}
    tab_of = {v: k for k, v in label.items()}
    say(f"lab seeded: {len(snap['workspaces'])} spaces, {len(snap['tabs'])} tabs, {len(snap['agents'])} agents")

    import shutil
    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start: {S.app('start').strip()}")
    want = expected()
    s = wait_lines(lambda s: len(s["sidebar_lines"]) >= len(want) and texts(s)[:1] == want[:1] and "▾ ◆ agent-rails  factory" in texts(s))
    if s is None:
        check("app state readable", False)
        return finish()
    # Start on the Rails space, on the recruiter lane (the default selection is the first lane).
    S.cmd({"cmd": "select", "tab": tab_of["recruiter"]})
    s = wait_lines(lambda s: texts(s) == want, 6)
    say("sidebar lines as drawn:")
    for l in s["sidebar_lines"]:
        say(f"  {l['kind']:12} {l['text']}")
    check("Rails space renders the mock's rows in the mock's order (current space's rows under its row; other spaces and hidden after) with the mock's fold state",
          texts(s) == want, diff(want, texts(s)))

    def line(s, text):
        return next((l for l in s["sidebar_lines"] if l["text"] == text), None)

    # Row facts the text does not carry.
    sel = [l["text"] for l in s["sidebar_lines"] if l["selected"]]
    check("selected rows: the current space and the selected lane", sel == ["▾ ◆ agent-rails  factory", "▸ ● recruiter  2 wf"], f"selected={sel}")
    kinds = [(l["text"], l["kind"]) for l in s["sidebar_lines"] if l["kind"] in ("orchestrator", "lane", "workflow")]
    check("row kinds: 1 orchestrator, 7 lanes, 3 workflow rows shown (of 8 under their lanes)",
          [k for _, k in kinds].count("orchestrator") == 1 and [k for _, k in kinds].count("lane") == 7
          and [k for _, k in kinds].count("workflow") == 3, f"{[k for _, k in kinds]}")
    check("lane rows carry no host badge (all on Studio); workflow rows carry PC",
          all(l["host"] is None for l in s["sidebar_lines"] if l["kind"] == "lane")
          and all(l["host"] == "PC" for l in s["sidebar_lines"] if l["kind"] == "workflow"))

    # Fold by hand: open, close, and a hand-set close beats the come-forward rule.
    S.cmd({"cmd": "sidebar_fold", "id": f"tab:{tab_of['recruiter']}", "open": True})
    s2 = wait_lines(lambda s: "▾ ● recruiter  2 wf" in texts(s))
    i = texts(s2).index("▾ ● recruiter  2 wf")
    check("opening a folded lane lists its workflows in order",
          texts(s2)[i + 1:i + 3] == ["  ● wf recruiter-2320  [PC]", "  ● wf recruiter-2315  [PC]"], f"{texts(s2)[i:i + 3]}")
    S.cmd({"cmd": "sidebar_fold", "id": f"tab:{tab_of['recruiter']}", "open": False})
    S.cmd({"cmd": "sidebar_fold", "id": f"tab:{tab_of['workspace embed']}", "open": False})
    s2 = wait_lines(lambda s: "▸ ● workspace embed ⟳  2 wf" in texts(s))
    check("a fold closed by hand stays closed although a workflow inside asks",
          s2 is not None and "▸ ● workspace embed ⟳  2 wf" in texts(s2) and "  ◐ wf embed wave-a  [PC]" not in texts(s2)
          and "▸ ● recruiter  2 wf" in texts(s2))
    S.cmd({"cmd": "sidebar_fold", "id": f"tab:{tab_of['workspace embed']}", "open": True})

    # Come forward: a workflow that asks opens its lane; back to idle folds it again (no manual entry).
    pane = PANES["wf infra 2"]
    herdr("pane", "report-agent", pane, "--source", "spike", "--agent", "codex", "--state", "blocked")
    s2 = wait_lines(lambda s: "▾ ● factory infra  3 wf" in texts(s))
    j = texts(s2).index("▾ ● factory infra  3 wf") if s2 and "▾ ● factory infra  3 wf" in texts(s2) else -1
    check("a blocked workflow opens its collapsed lane on its own, showing the asking row",
          j >= 0 and texts(s2)[j + 1:j + 4] == ["  ● wf infra 1  [PC]", "  ◐ wf infra 2  [PC]", "  ● wf infra 3  [PC]"],
          f"{texts(s2)[j:j + 4] if j >= 0 else 'lane still closed'}")
    herdr("pane", "report-agent", pane, "--source", "spike", "--agent", "codex", "--state", "working")
    s2 = wait_lines(lambda s: "▸ ● factory infra  3 wf" in texts(s))
    check("...and folds again once nothing inside asks (no hand-set entry)", s2 is not None and "▸ ● factory infra  3 wf" in texts(s2))

    # A failed workflow (state token) shows the failure glyph and opens its lane.
    fpane = PANES["wf blocks fold 5"]
    herdr("pane", "report-metadata", fpane, "--source", "spike", "--token", "state=failed")
    s2 = wait_lines(lambda s: "▾ ● agent blocks  2 wf" in texts(s))
    k = texts(s2).index("▾ ● agent blocks  2 wf") if s2 and "▾ ● agent blocks  2 wf" in texts(s2) else -1
    check("a failed workflow shows ✕ and opens its lane", k >= 0 and texts(s2)[k + 1] == "  ✕ wf blocks fold 5  [PC]",
          f"{texts(s2)[k:k + 2] if k >= 0 else 'lane still closed'}")
    herdr("pane", "report-metadata", fpane, "--source", "spike", "--clear-token", "state")

    # An advisor that asks leaves background and is a lane again.
    apane = PANES["step back"]
    herdr("pane", "report-agent", apane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    s2 = wait_lines(lambda s: "LANES  8 open" in texts(s))
    check("an advisor that asks leaves `background` and joins LANES (8 open; background 2)",
          s2 is not None and "◐ step back" in texts(s2) and "▸ background  2" in texts(s2)
          and "  toyo research · recruiting email ✓" in texts(s2), f"{texts(s2)[-4:] if s2 else ''}")
    herdr("pane", "report-agent", apane, "--source", "spike", "--agent", "claude", "--state", "idle")
    S.cmd({"cmd": "sidebar_fold", "id": "background", "open": True})
    s2 = wait_lines(lambda s: "▾ background  3" in texts(s))
    b = texts(s2).index("▾ background  3") if s2 and "▾ background  3" in texts(s2) else -1
    rows3 = texts(s2)[b + 1:b + 4] if b >= 0 else []
    check("opening background lists its three tabs in order and drops the summary line",
          len(rows3) == 3 and "step back" in rows3[0] and "toyo research" in rows3[1] and "recruiting email" in rows3[2]
          and not any(t.startswith("  step back ·") for t in texts(s2)), f"{rows3}")
    S.cmd({"cmd": "sidebar_fold", "id": "background", "open": False})

    # Other spaces' rows.
    lb_pane = PANES["agent-lb work"]
    herdr("pane", "report-agent", lb_pane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    s2 = wait_lines(lambda s: "▸ ◆ agent-lb  ● 1" in texts(s))
    check("another space whose row asks shows `● 1`", s2 is not None and "▸ ◆ agent-lb  ● 1" in texts(s2))
    herdr("pane", "report-agent", lb_pane, "--source", "spike", "--agent", "claude", "--state", "idle")
    s2 = wait_lines(lambda s: "▸ ◆ agent-lb  1" in texts(s))
    check("...and goes back to its count when nothing asks", s2 is not None and "▸ ◆ agent-lb  1" in texts(s2))

    S.cmd({"cmd": "sidebar_fold", "id": "hidden", "open": True})
    s2 = wait_lines(lambda s: "▾ hidden  3" in texts(s))
    h = texts(s2).index("▾ hidden  3") if s2 and "▾ hidden  3" in texts(s2) else -1
    check("opening `hidden` lists the three hidden spaces",
          h >= 0 and texts(s2)[h + 1:h + 4] == ["  ◇ archive-a  1", "  ◇ archive-b  1", "  ◇ archive-c  1"], f"{texts(s2)[h:h + 4] if h >= 0 else ''}")
    S.cmd({"cmd": "sidebar_fold", "id": "hidden", "open": False})

    # Space chord: cmd+shift+3 is homebase (plain: lanes only), cmd+shift+2 agent-lb, cmd+shift+1 back.
    S.cmd({"cmd": "activate"})
    time.sleep(0.3)
    S.key("3", ["cmd", "shift"])
    s2 = wait_lines(lambda s: "▾ ◆ homebase  plain" in texts(s))
    want_hb = ["SPACES  ⌘⇧1..9", "▸ ◆ agent-rails  ● 2", "▸ ◆ agent-lb  1", "▾ ◆ homebase  plain",
               "LANES  3 open", "● aside hls playback", "· dev server", "· notes", "▸ hidden  3"]
    check("cmd+shift+3 switches to the plain space: lanes only, no ORCHESTRATOR or WORKFLOWS, agent-rails shows `● 2` (wf reap, wf embed wave-a)",
          s2 is not None and texts(s2) == want_hb, diff(want_hb, texts(s2) if s2 else []))
    S.key("1", ["cmd", "shift"])
    s2 = wait_lines(lambda s: "▾ ◆ agent-rails  factory" in texts(s))
    check("cmd+shift+1 returns to agent-rails", s2 is not None and "▾ ◆ agent-rails  factory" in texts(s2))
    S.key("9", ["cmd", "shift"])
    time.sleep(0.3)
    s2 = S.state()
    check("cmd+shift+9 (no ninth space) changes nothing", "▾ ◆ agent-rails  factory" in texts(s2))
    # A click on a space row does what the chord does.
    lbrow = line(s2, "▸ ◆ agent-lb  1")
    check("space rows carry their space id (a click is not exercised here)", lbrow is not None and lbrow["space"] is not None)

    # Screenshots, one per appearance.
    S.cmd({"cmd": "select", "tab": tab_of["recruiter"]})
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        time.sleep(0.8)
        png = os.path.join(CHK, f"P10-{mode}.png")
        if os.path.exists(png):
            os.unlink(png)
        S.cmd({"cmd": "shot", "out": png})
        for _ in range(60):
            if os.path.exists(png) and os.path.getsize(png) > 0:
                break
            time.sleep(0.1)
        ok = os.path.exists(png) and os.path.getsize(png) > 1000
        check(f"screenshot {mode}: checks/P10-{mode}.png written", ok, png)
    try:
        from PIL import Image
        px = {}
        for mode in ("light", "dark"):
            im = Image.open(os.path.join(CHK, f"P10-{mode}.png")).convert("RGB")
            px[mode] = im.getpixel((120, im.height - 450))     # inside the sidebar, in the empty area below the rows
        check("the two screenshots differ in the sidebar (light vs dark)", px["light"] != px["dark"], f"{px}")
    except Exception as e:  # noqa: BLE001
        check("screenshot pixels readable", False, repr(e))
    finish()


def finish():
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
