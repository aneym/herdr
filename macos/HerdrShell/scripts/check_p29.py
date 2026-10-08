#!/usr/bin/env python3
"""P29 check: command-click opens a link; agent transitions raise in-app attention only, never an OS notification.

  HERDR_SHELL_SPACE=1 python3 scripts/check_p29.py [--out checks/P29.txt]

Lab `shellspike-p29`. Agent status is reported on plain shells (same as check_p26). The
app records opened_urls and does not open a browser. Herdr Shell posts no OS
notifications, dock badges or sounds on any platform (Alex 2026-10-08), so the TestHook
state must not contain the `notifications` or `dock_badge` keys at all through every
transition: the selected tab blocking, a parked tab blocking, a background tab blocking
and flipping, and a background tab finishing. Attention stays in the app: a background
tab going blocked shows the blocked dot and leads attention_order / attention_latest, a
parked tab never joins attention_order, and a finished background tab joins it.
"""
import json
import os
import shutil
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-p29"
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
    S.OUT = os.path.join(D0, "checks", "P29.txt")

URL = "https://example.com/p29"


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


def os_alerts(s):
    """OS-level alert keys present in the TestHook dump; the app must emit neither key at all."""
    if s is None:
        return {"state": None}
    return {k: s[k] for k in ("notifications", "dock_badge") if k in s}


def order(s):
    return (s or {}).get("attention_order") or []


def status_of(s, tab):
    def walk(rows):
        for r in rows or []:
            if r.get("tab") == tab:
                return r.get("status")
            found = walk(r.get("children"))
            if found:
                return found
        return None
    sb = s.get("sidebar") or {}
    return walk(sb.get("orchestrator")) or walk(sb.get("lanes")) or walk(sb.get("workflows"))


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
    say(f"HerdrShell P29 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
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
    ws = jherdr("workspace", "create", "--label", "p29", "--cwd", "/tmp", "--no-focus")
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

    look, look_pane = ws["tab"]["tab_id"], ws["root_pane"]["pane_id"]
    herdr("tab", "rename", look, "looking")
    bg, bg_pane = tab("background")
    parked, parked_pane = tab("parked one")
    fin, fin_pane = tab("finisher")
    for pane in (look_pane, bg_pane, parked_pane, fin_pane):
        agent(pane, "working")

    modes = {"version": 1, "tabs": {
        parked: {"mode": "parked", "at": "2026-10-03T00:00:00.000Z", "by": "p29", "note": "parked for p29"},
    }}
    with open(os.path.join(FIX, "modes.json"), "w") as f:
        json.dump(modes, f)
    os.environ["CONTROL_MODES"] = os.path.join(FIX, "modes.json")

    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start: {S.app('start').strip()}")

    s = wait_state(lambda s: status_of(s, bg) == "working" and status_of(s, look) == "working", 40)
    check("baseline is working before any transition", s is not None, "" if s is None else f"look={status_of(s, look)} bg={status_of(s, bg)}")
    if s is None:
        return finish()

    S.cmd({"cmd": "select", "tab": look})
    # In the Space the app is a real foreground app: make its window key, as when Alex looks at it.
    S.cmd({"cmd": "activate"})
    time.sleep(0.3)
    # Marks the offscreen window as key for the attention rule (agent-run never activates).
    S.cmd({"cmd": "mouse", "pane": look_pane, "action": "down", "col": 1, "row": 1})
    S.cmd({"cmd": "mouse", "pane": look_pane, "action": "up", "col": 1, "row": 1})
    check("no os alerts at baseline", not os_alerts(S.state()), f"{os_alerts(S.state())}")

    herdr("pane", "report-agent", look_pane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    time.sleep(1.5)
    s = S.state()
    check("selected key tab going blocked posts no os notification", not os_alerts(s), f"{os_alerts(s)}")
    herdr("pane", "report-agent", look_pane, "--source", "spike", "--agent", "claude", "--state", "working")

    herdr("pane", "report-agent", parked_pane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    time.sleep(1.5)
    s = S.state()
    check("parked tab going blocked posts no os notification", not os_alerts(s), f"{os_alerts(s)}")
    check("parked tab stays out of attention_order", parked not in order(s), f"{order(s)}")

    herdr("pane", "report-agent", bg_pane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    s = wait_state(lambda s: status_of(s, bg) == "blocked" and bg in order(s), 20)
    check("background tab going blocked shows the blocked dot",
          s is not None and status_of(s, bg) == "blocked", f"{status_of(s, bg) if s else None}")
    check("background blocked tab leads attention",
          s is not None and order(s)[:1] == [bg] and s.get("attention_latest") == bg,
          f"order={order(s)} latest={(s or {}).get('attention_latest')}")
    check("background tab going blocked posts no os notification", not os_alerts(s), f"{os_alerts(s)}")

    herdr("pane", "report-agent", bg_pane, "--source", "spike", "--agent", "claude", "--state", "working")
    time.sleep(0.3)
    herdr("pane", "report-agent", bg_pane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    time.sleep(2.0)
    s = S.state()
    check("two flips in 1s post no os notification", not os_alerts(s), f"{os_alerts(s)}")

    # herdr has no reported "done": an idle agent in a tab nobody has looked at reads as done.
    herdr("pane", "report-agent", fin_pane, "--source", "spike", "--agent", "claude", "--state", "idle")
    s = wait_state(lambda s: fin in order(s), 20)
    check("working to done on a background tab joins attention_order",
          s is not None and fin in order(s) and parked not in order(s), f"{order(s)}")
    check("working to done posts no os notification and no dock badge", not os_alerts(s), f"{os_alerts(s)}")

    S.cmd({"cmd": "select", "tab": look})
    time.sleep(0.4)
    # A typed "\n" does not submit; press Return so the url prints on its own row.
    S.cmd({"cmd": "type", "text": "printf 'https://example.com/p29\\n'"})
    S.cmd({"cmd": "key", "key": "return"})
    # Click the printed output row, not the echoed command line: the command line holds
    # `printf '...p29\n'`, and a link there resolves with the literal \n (p29%5Cn).
    def output_row(text):
        return next((i for i, line in enumerate(text.splitlines()) if line.strip() == URL), None)

    seen = ""
    t0 = time.time()
    while time.time() - t0 < 15:
        seen = herdr("pane", "read", look_pane, "--source", "visible")
        if output_row(seen) is not None:
            break
        time.sleep(0.2)
    row = output_row(seen)
    check("pane prints the url on its own row", row is not None, seen[-180:].replace("\n", " | "))
    row = row or 0
    rows = seen.splitlines()
    col = max(rows[row].find(URL), 0) if row < len(rows) else 0

    def link(u):
        return u[len("desk "):] if u.startswith("desk ") else u

    def new_links(s, n):
        return [link(u) for u in ((s or {}).get("opened_urls") or [])[n:]]

    before_click = len(S.state().get("opened_urls") or [])
    click = {"cmd": "mouse", "pane": look_pane, "col": col + 4, "row": row, "mods": ["cmd"]}
    S.cmd({**click, "action": "move"})
    time.sleep(0.15)
    S.cmd({**click, "action": "down"})
    S.cmd({**click, "action": "up"})
    s = wait_state(lambda s: len(s.get("opened_urls") or []) > before_click, 8)
    added = new_links(s, before_click)
    check("cmd-click opens exactly the printed link", added == [URL], f"{added}")

    before_sim = len((s or {}).get("opened_urls") or [])
    S.cmd({"cmd": "open_url_sim", "url": URL})
    s = wait_state(lambda s: len(s.get("opened_urls") or []) > before_sim, 8)
    added = new_links(s, before_sim)
    check("open_url_sim records exactly the link", added == [URL], f"{added}")

    shot("P29-links.png")
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
