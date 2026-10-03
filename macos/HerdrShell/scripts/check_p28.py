#!/usr/bin/env python3
"""P28 check: ⌘K / ⌘G quick switcher ranks tabs and lands on the one you pick.

  python3 scripts/check_p28.py [--out checks/P28.txt]

Lab `shellspike-p28`. Six tabs, two workspaces, one of them blocked. Asserts from
the state dump: an empty query lists the blocked tab first, "rec" ranks recruiter
ahead of rolodex-record, pick moves selected_tab, Esc closes. Screenshots of the
open switcher in light and dark.
"""
import json
import os
import shutil
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-p28"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LABDIR = os.path.expanduser(f"~/.cache/herdr-build/{os.environ['SHELL_LAB']}")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
CHK = os.path.dirname(S.OUT) if "--out" in sys.argv else os.path.join(D0, "checks")
if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "P28.txt")


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
    say(f"HerdrShell P28 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    # run.sh passes --herdr $LAB/bin/herdr under the long lab name; lab.py may keep
    # the server under a shorter dir so the socket fits. Point the long path at it.
    real = subprocess.run(["python3", os.path.join(D0, "scripts", "lab.py"), "env"], capture_output=True, text=True).stdout
    home = next(l.split("=", 1)[1] for l in real.splitlines() if l.startswith("HOME="))
    os.makedirs(os.path.join(LABDIR, "bin"), exist_ok=True)
    link = os.path.join(LABDIR, "bin", "herdr")
    target = os.path.join(os.path.dirname(home), "bin", "herdr")
    if os.path.realpath(target) != (os.path.realpath(link) if os.path.lexists(link) else ""):
        if os.path.lexists(link):
            os.unlink(link)
        if os.path.dirname(target) != os.path.dirname(link):
            os.symlink(target, link)
    subprocess.run(["defaults", "delete", f"herdr.shell.{os.environ['SHELL_LAB']}"], capture_output=True)
    old = jherdr("workspace", "list")["workspaces"]

    def workspace(label):
        return jherdr("workspace", "create", "--label", label, "--cwd", "/tmp", "--no-focus")

    def extra(ws, label):
        r = jherdr("tab", "create", "--workspace", ws, "--label", label, "--cwd", "/tmp", "--no-focus")
        return r["tab"]["tab_id"], r["root_pane"]["pane_id"]

    factory = workspace("factory")
    life = workspace("life")
    for w in old:
        herdr("workspace", "close", w["workspace_id"])
    fw = factory["workspace"]["workspace_id"]
    lw = life["workspace"]["workspace_id"]
    recruiter = factory["tab"]["tab_id"]
    herdr("tab", "rename", recruiter, "recruiter")
    rolodex, _ = extra(fw, "rolodex-record")
    _, _ = extra(fw, "billing")
    onboarding = life["tab"]["tab_id"]
    herdr("tab", "rename", onboarding, "onboarding")
    _, _ = extra(lw, "release-notes")
    stuck, stuck_pane = extra(lw, "stuck build")

    def agent(pane, state):
        for _ in range(200):
            if "%" in herdr("pane", "read", pane, "--source", "visible"):
                break
            time.sleep(0.05)
        herdr("pane", "report-agent", pane, "--source", "spike", "--agent", "claude", "--state", state)

    agent(stuck_pane, "blocked")

    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start: {S.app('start').strip()}")
    S.cmd({"cmd": "frame", "w": 1280, "h": 820})
    S.cmd({"cmd": "switcher", "open": True, "query": ""})
    s = wait_state(lambda s: s.get("switcher_open") and len(s.get("switcher_results") or []) >= 6, 40)
    check("switcher opens over the six tabs", s is not None and s.get("switcher_open") is True
          and len(s.get("switcher_results") or []) >= 6, f"{None if s is None else s.get('switcher_results')}")
    if s is None:
        return finish()
    res = s["switcher_results"]
    check("empty query lists the blocked tab first", res[0] == stuck, f"{res}")

    S.cmd({"cmd": "switcher", "query": "rec"})
    s = wait_state(lambda s: (s.get("switcher_results") or [None])[0] == recruiter, 10)
    res = (s or {}).get("switcher_results") or []
    check("query rec ranks recruiter ahead of rolodex-record",
          bool(res) and res[0] == recruiter and rolodex in res and res.index(recruiter) < res.index(rolodex),
          f"{res}")
    S.cmd({"cmd": "docs", "open": False})
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        time.sleep(0.6)
        shot(f"P28-switcher-{mode}.png")

    # Start from another tab, so the pick has to move the selection (the app selects the first lane at launch).
    S.cmd({"cmd": "select", "tab": stuck})
    wait_state(lambda s: s.get("selected_tab") == stuck, 8)
    S.cmd({"cmd": "switcher", "open": True})
    S.cmd({"cmd": "switcher", "query": "rec"})
    wait_state(lambda s: (s.get("switcher_results") or [None])[0] == recruiter, 8)
    S.cmd({"cmd": "switcher", "pick": 1})
    s = wait_state(lambda s: s.get("selected_tab") == recruiter, 10)
    check("pick moves selected_tab", s is not None and s.get("selected_tab") == recruiter,
          f"{None if s is None else s.get('selected_tab')}")

    S.cmd({"cmd": "switcher", "open": True})
    time.sleep(0.3)
    S.key("escape")
    s = wait_state(lambda s: s.get("switcher_open") is False, 8)
    check("Esc closes the switcher", s is not None and s.get("switcher_open") is False,
          f"{None if s is None else s.get('switcher_open')}")
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
