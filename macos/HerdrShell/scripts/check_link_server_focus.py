#!/usr/bin/env python3
import json
import pathlib
import socket
import time

import check_link_external as L

S = L.S
S.OUT = str(L.ROOT / "checks/LINK-SERVER-FOCUS.txt")
URL = "https://example.com/server-focus"
CASES = [("plain", "{url}", "{url}", True),
         ("osc8", "\\033]8;;{url}\\033\\\\osc8-label\\033]8;;\\033\\\\", "osc8-label", False)]


def probe(pane, row):
    try:
        return resolve(pane, row)
    except Exception as exc:
        return f"probe failed: {exc!r}"[:200]


def resolve(pane, row):
    import lab
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(3)
        s.connect(lab.SOCK)
        s.sendall((json.dumps({"id": "probe", "method": "pane.link.resolve",
                               "params": {"pane_id": pane, "viewport_row": row, "col": 2}}) + "\n").encode())
        return s.recv(65536).decode().strip()[:200]


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = L.api("workspace", "list")["workspaces"]
    made = L.api("workspace", "create", "--label", "links", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws, tab, pane = made["workspace"]["workspace_id"], made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    other = L.api("tab", "create", "--workspace", ws, "--label", "other")["tab"]["tab_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "select", "tab": other})
    L.wait(lambda s: s.get("window_key") is True and s.get("selected_tab") == other)
    time.sleep(1)
    S.lab("herdr", "tab", "focus", other)
    S.cmd({"cmd": "select", "tab": tab})
    state = L.wait(lambda s: s.get("window_key") is True and L.surface(s, pane).get("cols", 0) > 0)
    L.check("dev app is key in the Space and the link pane is attached",
            state.get("window_key") is True and bool(L.surface(state, pane)))
    S.wait_read(pane, lambda text: "%" in text, timeout=10)
    pid, window = L.app_window()
    for name, printed, shown, desk in CASES:
        url = f"{URL}-{name}"
        shown = shown.format(url=url)
        S.lab("herdr", "pane", "send-keys", pane, "C-c")
        S.lab("herdr", "pane", "run", pane, "clear; printf '" + printed.format(url=url) + "\\n'; sleep 3600")
        L.wait(lambda s: shown in [l.strip() for l in L.surface(s, pane).get("visible_nonblank", [])])
        visible = S.lab("herdr", "pane", "read", pane, "--source", "visible").splitlines()
        row = next((i for i, line in enumerate(visible) if line.strip() == shown), None)
        state = S.state()
        L.check(f"[{name}] the Shell shows the URL on the tab it selected",
                row is not None and bool(L.surface(state, pane).get("cols")), f"row={row}")
        if row is None:
            continue
        answer = probe(pane, row)
        L.check(f"[{name}] the server resolves links on the pane the Shell shows", '"regions"' in answer, answer)
        L.quit_browser()
        if desk:
            before = list(state.get("opened_urls", []))
            L.real_click(pid, window, state, pane, 2, row, ("cmd",))
            opened = L.opened_after(before, 1)
            L.check(f"[{name}] plain Cmd+click routes the URL to the desk", opened == ["desk " + url], f"opened={opened}")
            state = S.state()
        before = list(state.get("opened_urls", []))
        res = L.real_click(pid, window, state, pane, 2, row, ("cmd", "shift"))
        opened = L.opened_after(before, 1)
        L.check(f"[{name}] Cmd+Shift+click opens the URL in the default browser",
                opened == [url], f"opened={opened} click={res.get('text', '')[:200]}")
        L.restore_front()
        time.sleep(1)


if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        L.check("scenario completed", False, repr(exc)[:400])
    finally:
        try:
            S.app("stop")
            S.lab("down")
        finally:
            L.lines.append(f"{sum(l.startswith('[PASS]') for l in L.lines)} passed; {len(L.failures)} failed")
            pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
            pathlib.Path(S.OUT).write_text("\n".join(L.lines) + "\n")
    if L.failures:
        raise SystemExit("FAIL: " + ", ".join(L.failures))
