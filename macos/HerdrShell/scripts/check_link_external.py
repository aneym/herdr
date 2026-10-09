#!/usr/bin/env python3
"""Cmd+Shift+click on a terminal link or a desk web link opens the default browser.

Run: python3 scripts/check_link_external.py   (writes checks/LINK-EXTERNAL.txt)

Alex, 2026-10-09 ~10:20 ET: "cmd shift clicking links in herdr shell should open them in
my native browser". The dev build (.build/release/HerdrShell) runs in the Cua Space, never
on Alex's desktop, against an isolated lab server (SHELL_LAB=shellspike-lx, a copy of
HERDR_SHELL_BIN, default ~/.local/bin/herdr). A pane prints an OSC 8 link, a plain URL and
a file path. Clicks are real CGEvents posted by cua-driver in the guest with the modifier
keys held (foreground delivery, so macOS sees physical modifier state as for a hand on
the keyboard). The OS boundary is real: the external opener is NSWorkspace in the guest,
observed through the app's opened-link record (written just before NSWorkspace.open) and
the guest's default browser process, and the desk is read back from the lab server.
"""
import json
import os
import pathlib
import shlex
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-lx"
os.environ["HERDR_SHELL_SPACE"] = "1"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.setdefault("HERDR_SPACE_OWNER", "check-link-external")
ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/LINK-EXTERNAL.txt")
lines, failures = [], []
OSC_URL = "https://example.com/osc8-external"
PLAIN_URL = "https://example.com/plain-external"
DESK_URL = "https://example.com/desk-cmd"
FILE_PATH = "/tmp/herdr-link-external.txt"
BROWSER = "Safari"


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def note(text):
    print(text, flush=True)
    lines.append(text)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    return state


def surface(state, pane):
    return next((s for s in state.get("surfaces", []) if s["pane"] == pane), {})


def driver(*calls):
    args = []
    for tool, params in calls:
        args += [tool, json.dumps(params)]
    out = S.space("driver", *args)
    return [json.loads(line) for line in out.splitlines() if line.startswith("{")]


def guest(command):
    return subprocess.run([sys.executable, str(ROOT / "scripts/space.py"), "exec", command],
                          capture_output=True, text=True).stdout


def browser_running():
    return BROWSER in guest(f"pgrep -x {BROWSER} >/dev/null && echo {BROWSER} || true")


def quit_browser():
    guest(f"pkill -x {BROWSER} || true; pkill -x TextEdit || true")
    deadline = time.monotonic() + 10
    while browser_running() and time.monotonic() < deadline:
        time.sleep(0.3)


def restore_front():
    """The opened browser (or app) takes the guest's front: quit it and give the Shell the
    front back, so the next click is not spent activating the window. True if the
    browser ran."""
    deadline = time.monotonic() + 15
    while not browser_running() and time.monotonic() < deadline:
        time.sleep(0.3)
    ran = browser_running()
    quit_browser()
    S.cmd({"cmd": "activate"})
    wait(lambda s: s.get("window_key") is True and s.get("app_active") is True, 5)
    return ran


def app_window():
    pid = int(json.loads(S.space("status"))["app"]["pid"])
    wins = driver(("list_windows", {"pid": pid}))[0]["structured"]["windows"]
    win = max(wins, key=lambda w: w["bounds"]["width"] * w["bounds"]["height"])
    return pid, win["window_id"]


def desk_refs(tab):
    return json.dumps(json.loads(S.lab("herdr", "desk", "list", "--tab", tab, "--json")))


def real_click(pid, window, state, pane, col, row, mods):
    """One left click at a grid cell's centre, as CGEvents posted by cua-driver with `mods` held."""
    s = surface(state, pane)
    (cw, ch), (px, py) = s["cell_pt"], s["padding_pt"]
    x, y = window_pixel(state, s["window_rect"], px + (col + 0.5) * cw, py + (row + 0.5) * ch)
    # get_window_state anchors the pixel click in the same driver session.
    res = driver(("get_window_state", {"pid": pid, "window_id": window, "include_screenshot": True}),
                 ("click", {"pid": pid, "window_id": window, "x": x, "y": y,
                            "modifier": list(mods), "delivery_mode": "foreground"}))
    return res[-1]


def rows_of(pane):
    visible = S.lab("herdr", "pane", "read", pane, "--source", "visible").splitlines()
    find = lambda pred: next((i for i, line in enumerate(visible) if pred(line)), None)
    return (find(lambda l: l.strip() == "osc-label"), find(lambda l: l.strip() == PLAIN_URL),
            find(lambda l: l.strip() == FILE_PATH))


def opened_after(before, count, timeout=8):
    state = wait(lambda s: len(s.get("opened_urls", [])) >= len(before) + count, timeout)
    time.sleep(0.5)  # a duplicate open would land here
    return S.state().get("opened_urls", [])[len(before):]


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "links", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    tab, pane = made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "select", "tab": tab})
    state = wait(lambda s: s.get("window_key") is True and surface(s, pane).get("cols", 0) > 0)
    check("dev app is key in the Space and the lab pane is attached",
          state.get("window_key") is True and bool(surface(state, pane)))
    S.wait_read(pane, lambda text: "%" in text, timeout=10)
    S.space("exec", f"echo link-external > {FILE_PATH}")
    command = ("printf '\\033[2J\\033[H\\033]8;;" + OSC_URL + "\\033\\\\osc-label\\033]8;;\\033\\\\\\n"
               + PLAIN_URL + "\\n" + FILE_PATH + "\\n'; sleep 3600")
    S.lab("herdr", "pane", "run", pane, command)
    wait(lambda s: "osc-label" in [l.strip() for l in surface(s, pane).get("visible_nonblank", [])])
    osc_row, plain_row, file_row = rows_of(pane)
    check("pane shows the OSC 8 label, the plain URL and the file path",
          None not in (osc_row, plain_row, file_row), f"rows={osc_row},{plain_row},{file_row}")
    pid, window = app_window()
    quit_browser()
    desk_before = desk_refs(tab)
    ran = []

    def external(name, row, want):
        state = S.state()
        before = list(state.get("opened_urls", []))
        res = real_click(pid, window, state, pane, 2, row, ("cmd", "shift"))
        opened = opened_after(before, 1)
        check(name, opened == [want], f"opened={opened} click={res.get('text', '')[:200]}")
        ran.append(restore_front())

    external("Cmd+Shift+click on the OSC 8 label opens its target externally", osc_row, OSC_URL)
    external("Cmd+Shift+click on a plain URL opens it externally", plain_row, PLAIN_URL)
    check(f"the guest's default browser ({BROWSER}) ran after each external URL open", ran == [True, True], f"ran={ran}")
    external("Cmd+Shift+click on a file path opens it with the default app", file_row, "file://" + FILE_PATH)
    check("no Cmd+Shift click changed the pane's desk", desk_refs(tab) == desk_before, desk_refs(tab)[:300])
    state = wait(lambda s: s.get("docs_visible") is False, 2)
    check("the desk column stayed closed", state.get("docs_visible") is False)

    # Plain Cmd+click keeps today's behaviour: the link opens on the pane's desk.
    state = S.state()
    before = list(state.get("opened_urls", []))
    real_click(pid, window, state, pane, 2, osc_row, ("cmd",))
    opened = opened_after(before, 1)
    check("plain Cmd+click on the OSC 8 label routes to the desk", opened == ["desk " + OSC_URL], f"opened={opened}")
    deadline = time.monotonic() + 8
    while OSC_URL not in desk_refs(tab) and time.monotonic() < deadline:
        time.sleep(0.3)
    check("the Cmd+clicked URL is on the pane's desk", OSC_URL in desk_refs(tab), desk_refs(tab)[:300])
    desk_web(pid, window, tab)
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/LINK-EXTERNAL.png")})


def window_pixel(state, rect, dx, dy):
    """Screenshot pixel of a point `dx`,`dy` inside `rect` (window content points, top-left)."""
    frame = state["window_frame"].replace("{", "").replace("}", "").split(",")
    title = float(frame[3]) - state["content_height"]
    scale = state["backing_scale"]
    return round((rect[0] + dx) * scale), round((title + rect[1] + dy) * scale)


def desk_web(pid, window, tab):
    """A desk page's same-host link navigates the desk on a plain click and opens the
    default browser on Cmd+Shift+click, leaving the desk on its page."""
    port = 8765
    page = f"http://127.0.0.1:{port}/index.html"
    nxt = f"http://127.0.0.1:{port}/next.html"
    # One link fills the top of the page, so a click near the page's corner lands on it.
    body = "<a href=/next.html style='display:block;height:300px;font-size:40px'>NEXTLINK</a>"
    S.space("exec", "mkdir -p /tmp/lx-web && printf '%s' " + shlex.quote(body) + " > /tmp/lx-web/index.html"
            " && printf '%s' '<h1>next page</h1>' > /tmp/lx-web/next.html"
            " && (test -f /tmp/lx-web.pid && kill $(cat /tmp/lx-web.pid) 2>/dev/null; true)"
            f" && (nohup python3 -m http.server {port} --bind 127.0.0.1 --directory /tmp/lx-web"
            " >/dev/null 2>&1 & echo $! > /tmp/lx-web.pid) && sleep 1")
    S.lab("herdr", "desk", "open", "--tab", tab, page)
    state = wait(lambda s: (s.get("docs") or {}).get("url") == page and "NEXTLINK" in (s.get("docs") or {}).get("text", ""), 15)
    check("desk shows the local web page", "NEXTLINK" in (state.get("docs") or {}).get("text", ""),
          json.dumps(state.get("docs"))[:300])
    rect = (state.get("docs") or {}).get("web_rect") or []
    check("desk page has a frame in the window", len(rect) == 4 and rect[2] > 100, f"web_rect={rect}")
    if len(rect) != 4:
        return
    x, y = window_pixel(state, rect, 60, 60)
    before = list(S.state().get("opened_urls", []))
    driver(("get_window_state", {"pid": pid, "window_id": window, "include_screenshot": True}),
           ("click", {"pid": pid, "window_id": window, "x": x, "y": y,
                      "modifier": ["cmd", "shift"], "delivery_mode": "foreground"}))
    opened = opened_after(before, 1)
    check("Cmd+Shift+click on a desk page link opens it externally", opened == [nxt], f"opened={opened}")
    state = S.state()
    check("the desk stayed on its page", (state.get("docs") or {}).get("url") == page,
          (state.get("docs") or {}).get("url", ""))
    check(f"{BROWSER} ran for the desk link", restore_front())
    driver(("get_window_state", {"pid": pid, "window_id": window, "include_screenshot": True}),
           ("click", {"pid": pid, "window_id": window, "x": x, "y": y, "delivery_mode": "foreground"}))
    state = wait(lambda s: (s.get("docs") or {}).get("url") == nxt, 8)
    check("a plain click on the same link navigates the desk", (state.get("docs") or {}).get("url") == nxt,
          (state.get("docs") or {}).get("url", ""))
    S.space("exec", "kill $(cat /tmp/lx-web.pid) 2>/dev/null; rm -f /tmp/lx-web.pid; true")


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
