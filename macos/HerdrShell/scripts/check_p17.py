#!/usr/bin/env python3
"""P17 check: per-tab context, shared sign-in, browser chrome. Lab only, --agent-run.

  SHELL_LAB=shellspike-e python3 scripts/check_p17.py --out checks/P17.txt
"""
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

os.environ["SHELL_LAB"] = "shellspike-e"
os.environ.setdefault(
    "HERDR_SHELL_BIN",
    os.path.expanduser("~/.cache/herdr-build/shellspike-q/bin/herdr"),
)
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NAME = os.environ["SHELL_LAB"]
direct = os.path.expanduser(f"~/.cache/herdr-build/{NAME}")
sock = os.path.join(direct, "h", ".config", "herdr", "sessions", NAME, "herdr.sock")
LABDIR = direct if len(sock.encode()) <= 100 else os.path.expanduser("~/.cache/herdr-build/s/p17")
HOME = os.path.join(LABDIR, "h")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
FIX = os.path.join(LABDIR, "fixtures")
os.makedirs(FIX, exist_ok=True)
CTX = os.path.join(HOME, ".agent-rails", "herdr", "context")
CLI = os.path.join(D0, "bin", "herdr-context")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "P17.txt")
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


def pane_id_for(tab):
    panes = jherdr("pane", "list")["panes"]
    ids = [p["pane_id"] for p in panes if p.get("tab_id") == tab]
    if len(ids) != 1:
        raise SystemExit(f"lab pane list for {tab}: {ids}")
    return ids[0]


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


def cli_env(pane=None):
    env = {}
    for line in S.lab("env").splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            env[k] = v
    env["HERDR_CONTEXT_DIR"] = CTX
    env["HERDR_LANES_PATH"] = os.path.join(FIX, "lanes.json")
    env["HERDR_SESSION"] = NAME
    bindir = env.get("PATH", "").split(":")[0]
    if bindir:
        env["HERDR_BIN"] = os.path.join(bindir, "herdr")
    if pane:
        env["HERDR_PANE_ID"] = pane
    return env


def context(*args, pane=None):
    r = subprocess.run([CLI, *args], env=cli_env(pane), capture_output=True, text=True)
    return r.returncode, (r.stdout or "").strip(), (r.stderr or "").strip()


class Page(BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split("?", 1)[0]
        cookie = self.headers.get("Cookie") or ""
        if path == "/ref.html":
            title, body, extra = "Ref page", "ref ok", "p17=1; Path=/; Max-Age=86400"
        elif path == "/see.html":
            title = "cookie-yes" if "p17=1" in cookie else "cookie-no"
            body, extra = title, None
        elif path == "/second.html":
            title, body, extra = "Second page", "second ok", None
        else:
            title, body, extra = "P17", path, None
        html = f"<!doctype html><html><head><title>{title}</title></head><body>{body}</body></html>".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        if extra:
            self.send_header("Set-Cookie", extra)
        self.send_header("Content-Length", str(len(html)))
        self.end_headers()
        self.wfile.write(html)

    def log_message(self, *_a):
        pass


def main():
    if "--agent-run" not in sys.argv:
        sys.exit("refusing: check_p17 requires --agent-run")
    try:
        run()
    except Exception as e:  # noqa: BLE001
        say(f"[FAIL] check crashed -- {e!r}")
        failures.append("crashed")
        finish()


def run():
    say(f"HerdrShell P17 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    subprocess.run(["defaults", "delete", f"herdr.shell.{NAME}"], capture_output=True)
    shutil.rmtree(CTX, ignore_errors=True)
    srv = ThreadingHTTPServer(("127.0.0.1", 0), Page)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{port}"
    say(f"pages on {base}")

    S.lab("up")
    os.makedirs(CTX, exist_ok=True)
    note = os.path.join(FIX, "note.md")
    with open(note, "w") as f:
        f.write("# Note\n\nA context file.\n")
    old = jherdr("workspace", "list")["workspaces"]
    created = jherdr("workspace", "create", "--label", "ctx-space", "--cwd", "/tmp", "--no-focus")
    for w in old:
        herdr("workspace", "close", w["workspace_id"])
    ws = created["workspace"]["workspace_id"]
    tab_a = created["tab"]["tab_id"]
    pane_a = pane_id_for(tab_a)
    say(f"pane A {pane_a} from herdr pane list")
    made = jherdr("tab", "create", "--workspace", ws, "--label", "lane B", "--cwd", "/tmp", "--no-focus")
    tab_b = made["tab"]["tab_id"]
    herdr("tab", "rename", tab_a, "lane A")
    for pane in (pane_a,):
        for _ in range(200):
            if "%" in herdr("pane", "read", pane, "--source", "visible"):
                break
            time.sleep(0.05)

    lanes = {
        "version": 1, "generated_at": "2026-10-01T00:00:00Z",
        "lanes": [
            {"tab": tab_a, "name": "lane A", "label": "lane A", "kind": "lane",
             "goal": None, "goal_area": None, "section": "implementing", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
            {"tab": tab_b, "name": "lane B", "label": "lane B", "kind": "lane",
             "goal": None, "goal_area": None, "section": "implementing", "section_source": "project",
             "scope_url": None, "review_url": None, "mode": None},
        ],
    }
    areas = {
        "version": 1,
        "areas": [{"id": "ctx", "name": "ctx", "color": "#5AA9FF"}],
        "tabs": {tab_a: {"area": "ctx", "name": "lane A"}, tab_b: {"area": "ctx", "name": "lane B"}},
        "spaces": {}, "goal_area": {}, "goal": {},
    }
    with open(os.path.join(FIX, "lanes.json"), "w") as f:
        json.dump(lanes, f)
    with open(os.path.join(FIX, "areas.json"), "w") as f:
        json.dump(areas, f)
    os.environ["HERDR_LANES_PATH"] = os.path.join(FIX, "lanes.json")
    os.environ["HERDR_AREAS_PATH"] = os.path.join(FIX, "areas.json")
    os.environ["HERDR_CONTEXT_DIR"] = CTX

    code, out, err = context("add", f"{base}/ref.html", "--title", "Ref", pane=pane_a)
    check("herdr-context add from pane A's HERDR_PANE_ID", code == 0, err or out)
    code, out, err = context("add", note, "--title", "note.md", pane=pane_a)
    check("herdr-context add of a file from pane A", code == 0, err or out)

    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)

    def start():
        say(f"app start: {S.app('start').strip()}")
        S.cmd({"cmd": "frame", "w": 1440, "h": 900})
        S.cmd({"cmd": "docs", "open": True})

    start()
    s = wait_state(lambda s: any(l.get("title") == "lane A" for l in s["sidebar_lines"]), 40)
    check("app is up with lane A", s is not None)
    if s is None:
        finish()
        return

    click("row", "lane A")
    s = wait_state(lambda s: s.get("docs", {}).get("tabs") == ["Ref", "note.md"] and s["docs"].get("title") == "Ref page", 12)
    docs = (s or {}).get("docs") or {}
    check("row A shows the two context items in order",
          docs.get("tabs") == ["Ref", "note.md"], f"tabs={docs.get('tabs')}")
    check("the url item loads (and sets the cookie)", docs.get("title") == "Ref page", f"title={docs.get('title')!r}")

    click("row", "lane B")
    s = wait_state(lambda s: "Ref" not in (s.get("docs", {}).get("tabs") or []), 8)
    docs = (s or {}).get("docs") or {}
    check("row B does not show row A's items", "Ref" not in (docs.get("tabs") or []) and "note.md" not in (docs.get("tabs") or []),
          f"tabs={docs.get('tabs')}")

    click("row", "lane B")
    click("+")
    time.sleep(0.2)
    S.type_(f"{base}/see.html")
    S.key("return")
    s = wait_state(lambda s: "127.0.0.1" in (s.get("docs", {}).get("tabs") or []) and s["docs"].get("title") == "cookie-yes", 12)
    docs = (s or {}).get("docs") or {}
    check("a page in row B sees the cookie set in row A", docs.get("title") == "cookie-yes", f"title={docs.get('title')!r} tabs={docs.get('tabs')}")
    code, out, err = context("list", "--json", "--tab", tab_b)
    try:
        items = json.loads(out) if code == 0 else []
    except json.JSONDecodeError:
        items = []
    check("+ writes the item with added_by you",
          code == 0 and any(i.get("added_by") == "you" and i.get("kind") == "url" for i in items),
          err or out)

    click("row", "lane A")
    click("doc_tab", "Ref")
    s = wait_state(lambda s: s.get("docs", {}).get("title") == "Ref page", 8)
    click("address")
    time.sleep(0.2)
    S.type_(f"{base}/second.html")
    S.key("return")
    s = wait_state(lambda s: s.get("docs", {}).get("title") == "Second page", 10)
    check("typing in the address field navigates", (s or {}).get("docs", {}).get("title") == "Second page",
          f"title={(s or {}).get('docs', {}).get('title')!r} url={(s or {}).get('docs', {}).get('url')!r}")
    click("back")
    s = wait_state(lambda s: s.get("docs", {}).get("title") == "Ref page", 10)
    check("back returns to the first page", (s or {}).get("docs", {}).get("title") == "Ref page",
          f"title={(s or {}).get('docs', {}).get('title')!r}")

    click("web")
    time.sleep(0.2)
    S.key("l", ["cmd"])
    time.sleep(0.3)
    s = S.state()
    check("cmd+L focuses the address field when the panel has focus",
          (s.get("docs") or {}).get("address_focused") is True, f"docs={s.get('docs')}")

    S.cmd({"cmd": "select", "tab": tab_a})
    time.sleep(0.2)
    S.key("u", ["ctrl"])
    time.sleep(0.1)
    S.type_("echo p17-typed-ok")
    S.key("return")
    txt, dt = S.wait_read(pane_a, lambda x: any(l.strip() == "p17-typed-ok" for l in x.splitlines()), 8)
    check("with the panel open, typed text still reaches the focused pane",
          dt is not None, f"seen {dt:.3f}s" if dt is not None else txt[-300:])

    click("row", "lane B")
    click("doc_tab", "127.0.0.1")
    time.sleep(0.3)
    click("✕")
    s = wait_state(lambda s: "127.0.0.1" not in (s.get("docs", {}).get("tabs") or []), 8)
    code, out, err = context("list", "--json", "--tab", tab_b)
    try:
        left = json.loads(out) if code == 0 else None
    except json.JSONDecodeError:
        left = None
    check("✕ removes the item from the context file",
          left == [] and "127.0.0.1" not in ((s or {}).get("docs", {}).get("tabs") or []),
          f"file={out} tabs={(s or {}).get('docs', {}).get('tabs')}")

    # Put the shared page back, relaunch, and read the cookie again.
    code, out, err = context("add", f"{base}/see.html", "--title", "See", "--tab", tab_b)
    check("re-add the cookie page on row B before relaunch", code == 0, err or out)
    S.check_front(check)
    S.app("stop")
    time.sleep(0.4)
    start()
    click("row", "lane B")
    click("doc_tab", "See")
    s = wait_state(lambda s: s.get("docs", {}).get("title") in ("cookie-yes", "cookie-no"), 12)
    check("the cookie is still there after relaunch", (s or {}).get("docs", {}).get("title") == "cookie-yes",
          f"title={(s or {}).get('docs', {}).get('title')!r}")

    click("row", "lane A")
    s = wait_state(lambda s: s.get("docs", {}).get("tabs") == ["Ref", "note.md"], 8)
    png = os.path.join(CHK, "P17.png")
    if os.path.exists(png):
        os.unlink(png)
    S.cmd({"cmd": "frame", "w": 1440, "h": 900})
    time.sleep(0.4)
    S.cmd({"cmd": "shot", "out": png})
    for _ in range(80):
        if os.path.exists(png) and os.path.getsize(png) > 1000:
            break
        time.sleep(0.1)
    check("screenshot checks/P17.png with row A's two items",
          os.path.exists(png) and os.path.getsize(png) > 1000 and (s or {}).get("docs", {}).get("tabs") == ["Ref", "note.md"],
          png)
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
