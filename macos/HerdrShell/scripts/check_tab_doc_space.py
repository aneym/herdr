#!/usr/bin/env python3
import json
import os
import pathlib
import subprocess
import sys
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_tab_doc_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-doc"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.setdefault("HERDR_SPACE_OWNER", "check-tab-doc")
ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/TAB-DOC-SPACE.txt")
SHOTS = pathlib.Path(os.environ.get("TAB_DOC_SHOTS", str(ROOT / "checks")))
PINNED_URL = "https://example.com/rails/scope-review"
PLAIN_URL = "https://example.com/plain/doc"
lines, failures = [], []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=10):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    return state


def frame(state, key):
    return state.get("row_frames", {}).get(key)


def doc_frames(state):
    return sorted(k for k in state.get("row_frames", {}) if k.startswith("doc:"))


def app_window():
    pid = int(json.loads(S.space("status"))["app"]["pid"])
    out = S.space("driver", "list_windows", json.dumps({"pid": pid}))
    wins = [json.loads(l) for l in out.splitlines() if l.startswith("{")][0]["structured"]["windows"]
    win = max(wins, key=lambda w: w["bounds"]["width"] * w["bounds"]["height"])
    return pid, win["window_id"], win["bounds"]


def click_at(pid, window, state, x, y):
    scale = state["backing_scale"]
    S.space("driver", "get_window_state", json.dumps({"pid": pid, "window_id": window, "include_screenshot": True}),
            "click", json.dumps({"pid": pid, "window_id": window, "x": round(x * scale), "y": round(y * scale),
                                 "delivery_mode": "foreground"}))


def hover_at(bounds, x, y):
    x, y = bounds["x"] + x, bounds["y"] + y
    js = "ObjC.import('CoreGraphics');" + "".join(
        f"$.CGEventPost($.kCGHIDEventTap, $.CGEventCreateMouseEvent(null, $.kCGEventMouseMoved, $.CGPointMake({x + d}, {y}), 0));delay(0.05);"
        for d in (-4, -2, 0))
    S.space("exec", "osascript -l JavaScript -e " + json.dumps(js))


def quit_browsers():
    S.space("exec", "pkill -x Safari || true; pkill -x Aside || true")


def own_pane_set(pane, url):
    import lab
    return subprocess.run([lab.BIN, "--session", lab.SESSION, "tab", "doc", "set", url],
                          env={**lab.env(), "HERDR_PANE_ID": pane}, capture_output=True, text=True)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "rails", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws, rails = made["workspace"]["workspace_id"], made["tab"]["tab_id"]
    S.lab("herdr", "tab", "rename", rails, "Working in Rails")
    plain, bare = [api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
                   for name in ("plain doc", "no doc")]
    S.lab("herdr", "tab", "pin", rails)
    panes = {p["tab_id"]: p["pane_id"] for p in api("pane", "list")["panes"]}

    out = S.lab("herdr", "tab", "doc", "set", rails, PINNED_URL, "--title", "Rails scope").strip()
    check("CLI: tab doc set names a tab and prints tab, url and title",
          out == f"{rails} {PINNED_URL} Rails scope", out)
    res = own_pane_set(panes[plain], PLAIN_URL)
    check("CLI: tab doc set with no tab targets the caller's own tab (HERDR_PANE_ID)",
          res.returncode == 0 and res.stdout.strip().startswith(f"{plain} {PLAIN_URL}"), res.stdout + res.stderr)
    got = json.loads(S.lab("herdr", "tab", "doc", "get", "--tab", bare, "--json"))
    check("CLI: tab doc get on a tab without a doc reports none", got.get("doc") is None, json.dumps(got))
    listed = {t["tab_id"]: t.get("doc") for t in api("tab", "list")["tabs"]}
    check("tab.list carries each tab's doc", (listed.get(rails) or {}).get("url") == PINNED_URL
          and (listed.get(plain) or {}).get("url") == PLAIN_URL and listed.get(bare) is None, json.dumps(listed))

    S.app("start")
    S.cmd({"cmd": "activate"})
    pinned_row, plain_row, bare_row = "pinned:" + rails, "tab:" + plain, "tab:" + bare
    state = wait(lambda s: s.get("window_key") is True and rails in s.get("tab_docs", {})
                 and frame(s, pinned_row) and frame(s, plain_row), 30)
    check("the app reads both docs from the server", set(state.get("tab_docs", {})) == {rails, plain},
          json.dumps(state.get("tab_docs")))
    pid, window, bounds = app_window()

    def hover(key):
        f = frame(S.state(), key)
        if f:
            hover_at(bounds, f[0] + 40, f[1] + f[3] / 2)
        return f

    def away():
        cap = next(iter(S.state().get("pane_caps", [])), None)
        if cap:
            f = cap["window_frame"]
            hover_at(bounds, f[0] + f[2] / 2, f[1] + f[3] + 120)

    rails_row = "tab:" + rails
    S.cmd({"cmd": "select", "tab": bare})
    away()
    at_rest = ["doc:" + pinned_row, "doc:" + rails_row]
    state = wait(lambda s: doc_frames(s) == sorted(at_rest), 4)
    check("at rest the pinned tab's rows show its doc icon and unpinned rows show none",
          doc_frames(state) == sorted(at_rest), json.dumps(doc_frames(state)))

    hover(pinned_row)
    state = wait(lambda s: frame(s, "doc:" + pinned_row) is not None)
    check("hovering the pinned Working in Rails row keeps its doc icon", frame(state, "doc:" + pinned_row) is not None,
          json.dumps(doc_frames(state)))
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        state = wait(lambda s: s.get("theme", {}).get("effective") == mode)
        hover(pinned_row)
        wait(lambda s: frame(s, "doc:" + pinned_row) is not None, 4)
        time.sleep(0.5)
        state = S.state()
        png = SHOTS / f"TAB-DOC-SPACE-{mode}.png"
        png.parent.mkdir(parents=True, exist_ok=True)
        S.cmd({"cmd": "shot", "out": str(png), "scale": 2})
        time.sleep(1.5)
        check(f"{mode}: Shell window shot written with the icon showing",
              png.exists() and png.stat().st_size > 0 and frame(state, "doc:" + pinned_row) is not None)
    S.cmd({"cmd": "appearance", "mode": "light"})

    hover(pinned_row)
    state = wait(lambda s: frame(s, "doc:" + pinned_row) is not None)
    before = list(state.get("opened_urls", []))
    f = frame(state, "doc:" + pinned_row)
    click_at(pid, window, state, f[0] + f[2] / 2, f[1] + f[3] / 2)
    state = wait(lambda s: len(s.get("opened_urls", [])) > len(before))
    opened = state.get("opened_urls", [])[len(before):]
    check("clicking the icon opens the doc in Aside", opened == ["aside " + PINNED_URL], json.dumps(opened))
    check("clicking the icon leaves the selected tab alone", state.get("selected_tab") == bare, str(state.get("selected_tab")))
    time.sleep(2)
    quit_browsers()
    S.cmd({"cmd": "activate"})

    hover(plain_row)
    state = wait(lambda s: frame(s, "doc:" + plain_row) is not None)
    check("an unpinned tab row shows its doc icon on hover", frame(state, "doc:" + plain_row) is not None,
          json.dumps(doc_frames(state)))
    away()
    state = wait(lambda s: frame(s, "doc:" + plain_row) is None, 4)
    check("the unpinned row's icon goes when the pointer leaves", frame(state, "doc:" + plain_row) is None,
          json.dumps(doc_frames(state)))
    hover(bare_row)
    time.sleep(0.8)
    state = S.state()
    check("a row with no doc shows no icon on hover", frame(state, "doc:" + bare_row) is None
          and doc_frames(state) == sorted(at_rest), json.dumps(doc_frames(state)))

    out = S.lab("herdr", "tab", "doc", "clear", rails).strip()
    check("CLI: tab doc clear", out == f"{rails} none", out)
    state = wait(lambda s: rails not in s.get("tab_docs", {}))
    hover(pinned_row)
    state = wait(lambda s: not doc_frames(s), 4)
    check("after clear the pinned row shows no doc icon, at rest or on hover", not doc_frames(state),
          json.dumps(doc_frames(state)))


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
