#!/usr/bin/env python3
"""P16 check: the docs column beside the pane.

  SHELL_LAB=shellspike-areas python3 scripts/check_p16.py [--out checks/P16.txt]

A local page is the scope URL. RESUME.md and BRIEF.md live in the project folder
the scope route names. Selecting the row shows Scope, RESUME, BRIEF. Typed text
still reaches the focused pane. Editing RESUME.md shows up within 6s. ⌘\\ hides
and shows the column, and the pane host width changes.
"""
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

os.environ["SHELL_LAB"] = "shellspike-areas"
os.environ.setdefault(
    "HERDR_SHELL_BIN",
    os.path.expanduser("~/.cache/herdr-build/shellspike-q/bin/herdr"),
)
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NAME = os.environ["SHELL_LAB"]
direct = os.path.expanduser(f"~/.cache/herdr-build/{NAME}")
sock = os.path.join(direct, "h", ".config", "herdr", "sessions", NAME, "herdr.sock")
LABDIR = direct if len(sock.encode()) <= 100 else os.path.expanduser("~/.cache/herdr-build/s/areas")
HOME = os.path.join(LABDIR, "h")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
FIX = os.path.join(LABDIR, "fixtures")
os.makedirs(FIX, exist_ok=True)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "P16.txt")
CHK = os.path.dirname(S.OUT)


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


def click(target, label=None):
    body = {"cmd": "click", "target": target}
    if label is not None:
        body["label"] = label
    S.cmd(body)
    time.sleep(0.45)


def host_w(s):
    nums = [float(x) for x in s["host_frame"].replace("{", " ").replace("}", " ").replace(",", " ").split()]
    return nums[2]


class Page(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"<!doctype html><html><head><title>P16 Scope</title></head><body>scope page ok</body></html>"
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_a):
        pass


def main():
    try:
        run()
    except Exception as e:  # noqa: BLE001
        say(f"[FAIL] check crashed -- {e!r}")
        failures.append("crashed")
        finish()


def run():
    say(f"HerdrShell P16 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    subprocess.run(["defaults", "delete", "herdr.shell.shellspike-areas"], capture_output=True)
    srv = ThreadingHTTPServer(("127.0.0.1", 0), Page)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    say(f"scope page on http://127.0.0.1:{port}/")

    S.lab("up")
    # lab up replaces HOME, so the project folder is written after that.
    folder = os.path.join(HOME, ".agent-rails", "scoping", "p16doc")
    os.makedirs(folder, exist_ok=True)
    resume = os.path.join(folder, "RESUME.md")
    with open(resume, "w") as f:
        f.write("# Resume\n\n| Alpha | Beta |\n| --- | --- |\n| one | two |\n")
    with open(os.path.join(folder, "BRIEF.md"), "w") as f:
        f.write("# Brief\n\nA short brief.\n")
    old = jherdr("workspace", "list")["workspaces"]
    created = jherdr("workspace", "create", "--label", "docs-space", "--cwd", "/tmp", "--no-focus")
    for w in old:
        herdr("workspace", "close", w["workspace_id"])
    tab = created["tab"]["tab_id"]
    pane = created["root_pane"]["pane_id"]
    herdr("tab", "rename", tab, "doc lane")
    for _ in range(200):
        if "%" in herdr("pane", "read", pane, "--source", "visible"):
            break
        time.sleep(0.05)

    scope = f"http://127.0.0.1:{port}/scope.html?route=scoping/p16doc"
    lanes = {
        "version": 1,
        "generated_at": "2026-10-01T00:00:00Z",
        "lanes": [{
            "tab": tab, "name": "doc lane", "label": "doc lane", "kind": "lane",
            "goal": None, "goal_area": None, "section": "scoping", "section_source": "project",
            "scope_url": scope, "review_url": None, "mode": None,
        }],
    }
    areas = {
        "version": 1,
        "areas": [{"id": "docs", "name": "docs", "color": "#5AA9FF"}],
        "tabs": {tab: {"area": "docs", "name": "doc lane"}},
        "spaces": {}, "goal_area": {}, "goal": {},
    }
    with open(os.path.join(FIX, "lanes.json"), "w") as f:
        json.dump(lanes, f)
    with open(os.path.join(FIX, "areas.json"), "w") as f:
        json.dump(areas, f)
    os.environ["HERDR_LANES_PATH"] = os.path.join(FIX, "lanes.json")
    os.environ["HERDR_AREAS_PATH"] = os.path.join(FIX, "areas.json")

    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start: {S.app('start').strip()}")
    S.cmd({"cmd": "frame", "w": 1440, "h": 900})
    s = wait_state(lambda s: s.get("shell", {}).get("mode") == "areas" and any(l.get("title") == "doc lane" for l in s["sidebar_lines"]), 40)
    check("app is up in Areas with the doc row", s is not None, None if s is None else "doc lane in sidebar")
    if s is None:
        finish()
        return

    time.sleep(0.4)
    click("row", "doc lane")
    s = wait_state(lambda s: s.get("docs", {}).get("tabs") == ["Scope", "RESUME", "BRIEF"] and s["docs"].get("title") == "P16 Scope", 20)
    docs = (s or {}).get("docs", {})
    say(f"docs after select: tabs={docs.get('tabs')} active={docs.get('active')} title={docs.get('title')!r} text={(docs.get('text') or '')[:180]!r}")
    check("selecting the row shows tabs Scope, RESUME, BRIEF",
          docs.get("tabs") == ["Scope", "RESUME", "BRIEF"] and docs.get("active") == "Scope",
          f"tabs={docs.get('tabs')} active={docs.get('active')}")
    check("Scope web view loads the served page (title read back)",
          docs.get("title") == "P16 Scope" and "scope page ok" in (docs.get("text") or ""),
          f"title={docs.get('title')!r} text={(docs.get('text') or '')[:120]!r}")

    click("doc_tab", "RESUME")
    s = wait_state(lambda s: "Resume" in (s.get("docs", {}).get("text") or "") and "Alpha" in (s.get("docs", {}).get("text") or "") and "one" in (s.get("docs", {}).get("text") or ""), 15)
    text = ((s or {}).get("docs") or {}).get("text") or ""
    check("RESUME renders a heading and a table",
          "Resume" in text and "Alpha" in text and "one" in text and ((s or {}).get("docs") or {}).get("active") == "RESUME",
          f"active={((s or {}).get('docs') or {}).get('active')} text={text[:200]!r}")

    # The web view finishes loading after the tab click. Put the pane back in front
    # and clear anything a focus change left on the line, then type.
    S.cmd({"cmd": "select", "tab": tab})
    time.sleep(0.3)
    S.key("u", ["ctrl"])
    time.sleep(0.15)
    S.type_("echo p16-typed-ok")
    S.key("return")
    txt, dt = S.wait_read(pane, lambda x: any(l.strip() == "p16-typed-ok" for l in x.splitlines()), 8)
    check("with the panel open, typed text still reaches the focused pane (herdr pane read)",
          dt is not None, f"seen {dt:.3f}s after the last key" if dt is not None else txt[-300:])

    S.cmd({"cmd": "select", "tab": tab})
    time.sleep(0.2)
    S.type_("cat -v")
    S.key("return")
    S.wait_read(pane, lambda x: any(l.strip() == "cat -v" for l in x.splitlines()), 8)
    time.sleep(0.2)
    S.cmd({"cmd": "select", "tab": tab})
    time.sleep(0.2)
    S.type_("z")
    S.key("return")
    txt, dt = S.wait_read(pane, lambda x: any(l.strip() == "z" for l in x.splitlines()), 8)
    check("with the panel open, cat -v echoes the typed character",
          dt is not None and "z" in txt, f"tail={[l for l in txt.splitlines() if l.strip()][-4:]}")
    S.key("c", ["ctrl"])
    time.sleep(0.2)

    with open(resume, "a") as f:
        f.write("\nP16-EDITED\n")
    s = wait_state(lambda s: "P16-EDITED" in (s.get("docs", {}).get("text") or ""), 6)
    edited = "P16-EDITED" in (((s or {}).get("docs") or {}).get("text") or "")
    check("editing RESUME.md updates the panel within 6s", edited,
          f"text tail={(((s or {}).get('docs') or {}).get('text') or '')[-120:]!r}")

    s = S.state()
    open_w = host_w(s)
    open_doc = s["shell"]["doc_open"]
    S.key("\\", ["cmd"])
    hidden = wait_state(lambda s: s["shell"]["doc_open"] is False, 5)
    hidden_w = host_w(hidden) if hidden else 0
    S.key("\\", ["cmd"])
    s = wait_state(lambda s: s["shell"]["doc_open"] is True, 5)
    shown_w = host_w(s) if s else 0
    check("cmd+\\ hides and shows the doc panel",
          open_doc is True and hidden is not None and hidden["shell"]["doc_open"] is False
          and s is not None and s["shell"]["doc_open"] is True,
          f"open={open_doc} hidden={None if hidden is None else hidden['shell']['doc_open']} shown={None if s is None else s['shell']['doc_open']}")
    # Re-read the hidden transition from the widths: hidden host is wider, shown host is narrower again.
    check("pane host width changes when the doc panel hides and shows",
          hidden_w > open_w + 50 and shown_w < hidden_w - 50,
          f"host width open={open_w:.0f} hidden={hidden_w:.0f} shown={shown_w:.0f}")

    png = os.path.join(CHK, "P16.png")
    if os.path.exists(png):
        os.unlink(png)
    S.cmd({"cmd": "frame", "w": 1440, "h": 900})
    time.sleep(0.4)
    S.cmd({"cmd": "shot", "out": png})
    for _ in range(80):
        if os.path.exists(png) and os.path.getsize(png) > 1000:
            break
        time.sleep(0.1)
    check("screenshot checks/P16.png", os.path.exists(png) and os.path.getsize(png) > 1000, png)
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
