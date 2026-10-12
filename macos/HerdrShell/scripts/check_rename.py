#!/usr/bin/env python3
"""R273: a tab renamed in Herdr Shell shows its new name at once and keeps it.

  python3 scripts/check_rename.py

Alex, 2026-10-07: "renaming this thread didnt work? report to herdr ui". One scenario on a lab
herdr server (the installed binary, isolated by lab.py; the live session is never touched):

1. The Shell's own request (`tab.rename` over the socket, HerdrClient's body) renames a lane tab;
   `tab list` shows the new name.
2. The pane's agent reports state and sets an OSC window title; the label stays.
3. The server stops and starts cold (a reload or reconnect); the label is still the new one.
4. The spaces tree row built from that snapshot shows the new name while overlay.json still
   carries the old one: the overlay writer copies labels every 30 s (32 s measured on Studio).

origin/main before the fix fails step 4: the row keeps the overlay's old name.
"""
import json
import os
import pathlib
import socket
import subprocess
import sys
import tempfile
import time

os.environ["SHELL_LAB"] = "shellspike-rename"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "Sources/HerdrShell"
sys.path.insert(0, str(ROOT / "scripts"))
import lab  # noqa: E402

NEW = "chauhan recap"
failures = []


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""), flush=True)
    if not ok:
        failures.append(name)


def j(*args):
    return json.loads(lab.herdr(*args))["result"]


def label(tab):
    return next((t.get("label") for t in j("tab", "list")["tabs"] if t["tab_id"] == tab), None)


def shell_call(method, params):
    """The request HerdrClient.call sends: one JSON line with a shell: id, one reply line."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(5)
        s.connect(lab.SOCK)
        s.sendall((json.dumps({"id": f"shell:{method}", "method": method, "params": params}) + "\n").encode())
        data = b""
        while not data.endswith(b"\n"):
            chunk = s.recv(65536)
            if not chunk:
                break
            data += chunk
    return json.loads(data)


def wait(pred, timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        if pred():
            return True
        time.sleep(0.1)
    return pred()


def main():
    build = pathlib.Path.home() / ".cache/herdr-build/rename-check"
    build.mkdir(parents=True, exist_ok=True)
    driver = build / "dump"
    subprocess.run(["swiftc", "-parse-as-library", *(str(SRC / f) for f in ("Snapshot.swift", "SpacesTree.swift", "DeskModel.swift")),
                    str(ROOT / "scripts/rename_dump.swift"), "-o", str(driver)], check=True)

    subprocess.run([sys.executable, str(ROOT / "scripts/lab.py"), "down"], capture_output=True)
    time.sleep(0.5)
    subprocess.run([sys.executable, str(ROOT / "scripts/lab.py"), "up"], check=True, capture_output=True)
    try:
        tabs = j("tab", "list")["tabs"]
        rec = next(t["tab_id"] for t in tabs if t.get("label") == "recruiter")
        pane = next(p["pane_id"] for p in j("pane", "list")["panes"] if p["tab_id"] == rec)

        reply = shell_call("tab.rename", {"tab_id": rec, "label": NEW})
        check("the Shell's tab.rename request succeeds", "error" not in reply, json.dumps(reply)[:200])
        check("tab list shows the new name", label(rec) == NEW, str(label(rec)))

        lab.herdr("pane", "report-agent", pane, "--source", "spike", "--agent", "claude", "--state", "working")
        lab.herdr("pane", "run", pane, "printf '\\033]0;claude: other title\\007'")
        titled = wait(lambda: "other title" in json.dumps(j("pane", "get", pane)))
        check("the agent's report and OSC title reach the server", titled)
        check("they leave the renamed label alone", label(rec) == NEW, str(label(rec)))

        lab.herdr("server", "stop", check=False)
        wait(lambda: not os.path.exists(lab.SOCK) or subprocess.run([lab.BIN, "--session", lab.SESSION, "workspace", "list"], env=lab.env(), capture_output=True).returncode != 0)
        time.sleep(0.5)
        lab.spawn_detached([lab.BIN, "--session", lab.SESSION, "server"], os.path.join(lab.LAB, "server.log"))
        up = wait(lambda: os.path.exists(lab.SOCK) and subprocess.run([lab.BIN, "--session", lab.SESSION, "workspace", "list"], env=lab.env(), capture_output=True).returncode == 0, 20)
        check("the lab server comes back after a cold stop", up)
        check("the new name survives the restart", label(rec) == NEW, str(label(rec)))

        snapshot = j("api", "snapshot")["snapshot"]
        with tempfile.TemporaryDirectory(prefix="rename-check-") as tmp:
            tmp = pathlib.Path(tmp)
            (tmp / "snapshot.json").write_text(json.dumps(snapshot))
            # What the overlay writer held when the rename landed: the name copied from the old label.
            (tmp / "overlay.json").write_text(json.dumps({"version": 1, "tabs": {
                rec: {"kind": "lane", "name": "recruiter", "section": "implementing"}}}))
            out = subprocess.check_output([str(driver), str(tmp / "snapshot.json"), str(tmp / "overlay.json")], text=True)
        rows = {tuple(line.split("|", 2)[:2]): line.split("|", 2)[2] for line in out.splitlines()}
        check("the spaces tree row shows the new name over the stale overlay name",
              rows.get(("spaces", rec)) == NEW, str(rows.get(("spaces", rec))))
    finally:
        subprocess.run([sys.executable, str(ROOT / "scripts/lab.py"), "down"], capture_output=True)
    print(f"{'FAIL' if failures else 'PASS'} check_rename ({len(failures)} failed)")
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()
