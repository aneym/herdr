#!/usr/bin/env python3
"""Another machine's pinned chats, through the app against two lab servers (H0 findings 7, 8, 13).

  python3 scripts/check_remote_pins.py --host-ok

Host run only, offscreen (--agent-run), like check_machines.py: it needs two lab sockets and the
Cua Space bridges one. Agents run Herdr Shell in the Space (Alex, 2026-10-02), so pass --host-ok
only on Alex's say until this harness is ported.

Lab A (SHELL_LAB=shellspike-h0a) is the local session; lab B (shellspike-h0b) stands in for
another machine, named `mlab` in the app's machines file. Two more machines, `MLAB` and `pcx`,
point at nothing and stay offline. Checks:
  - a pinned tab on mlab shows the state glyph and tone its machine-block row draws;
  - its row menu offers Unpin and an unpinned mlab tab offers Pin; the pin action (the call the
    menu item makes) changes the pin on lab B, not on lab A;
  - host footer rows keep `mlab` and `MLAB` apart; a case-only match still takes the one
    machine's spelling (PCX becomes pcx), and an ambiguous one (Mlab) is left alone.
Both labs are stopped at the end. Writes checks/REMOTE-PINS.txt.
"""
import json
import os
import pathlib
import subprocess
import sys
import time

os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.pop("HERDR_SHELL_SPACE", None)
ROOT = pathlib.Path(__file__).resolve().parents[1]
LOCAL, REMOTE = "shellspike-h0a", "shellspike-h0b"
os.environ["SHELL_LAB"] = LOCAL
sys.path.insert(0, str(ROOT / "scripts"))
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/REMOTE-PINS.txt")


def lab_b(*args):
    env = dict(os.environ, SHELL_LAB=REMOTE)
    return subprocess.run([sys.executable, str(ROOT / "scripts/lab.py"), *args], env=env,
                          capture_output=True, text=True).stdout


def b_snapshot():
    return json.loads(lab_b("herdr", "api", "snapshot"))["result"]["snapshot"]


def wait(pred, timeout=30):
    deadline = time.monotonic() + timeout
    st = {}
    while time.monotonic() < deadline:
        st = S.state()
        if pred(st):
            return st
        time.sleep(0.3)
    return st


def row(rows, row_id):
    return next((r.split("|") for r in rows if r.split("|")[1] == row_id), None)


def main():
    if "--host-ok" not in sys.argv:
        raise SystemExit("check_remote_pins opens the app on this host; pass --host-ok (see the docstring)")
    lab_b("up")
    b_sock = next(line.split("=", 1)[1] for line in lab_b("env").splitlines() if line.startswith("HERDR_SOCKET_PATH="))
    snap = b_snapshot()
    working = {a["tab_id"] for a in snap["agents"] if a.get("agent_status") == "working"}
    agents = {a["tab_id"] for a in snap["agents"]}
    pin_tab = next(t["tab_id"] for t in snap["tabs"] if t["tab_id"] in working)
    other = next(t["tab_id"] for t in snap["tabs"] if t["tab_id"] in agents - working)
    if next(t for t in snap["tabs"] if t["tab_id"] == pin_tab).get("pin_index") is None:
        lab_b("herdr", "tab", "pin", pin_tab)

    S.app("stop")
    S.lab("down")
    S.lab("up")
    lab_dir = pathlib.Path(S.lab("env").split("HOME=", 1)[1].splitlines()[0]).parent
    app_dir = pathlib.Path.home() / ".cache/herdr-build" / LOCAL
    (app_dir / "bin").mkdir(parents=True, exist_ok=True)
    link = app_dir / "bin/herdr"
    if not link.exists():
        link.symlink_to(lab_dir / "bin/herdr")
    machines = lab_dir / "machines.json"
    machines.write_text(json.dumps({"machines": [
        {"name": "mlab", "socket": b_sock},
        {"name": "MLAB", "dir": str(lab_dir / "no-such-machine")},
        {"name": "pcx", "dir": str(lab_dir / "no-such-machine-2")},
    ]}))
    overlay = lab_dir / "overlay" / "overlay.json"
    overlay.parent.mkdir(exist_ok=True)
    overlay.write_text(json.dumps({"version": 1, "hosts": [{"name": n} for n in ("mlab", "MLAB", "Mlab", "PCX")]}))
    os.environ["HERDR_SHELL_MACHINES"] = str(machines)
    os.environ["FACTORY_OVERLAY"] = str(overlay)
    S.app("start", "--host-ok")

    pinned_id, tab_id = f"pinned:mlab/{pin_tab}", f"tab:mlab/{pin_tab}"
    st = wait(lambda s: row(s.get("spaces_rows", []), pinned_id) and row(s.get("spaces_rows", []), tab_id), 40)
    rows = st.get("spaces_rows", [])
    pinned, block = row(rows, pinned_id), row(rows, tab_id)
    S.check("remote pinned row shows the machine row's state",
            bool(pinned and block) and pinned[4:6] == block[4:6] == ["●", "working"],
            f"pinned {pinned and pinned[4:6]} machine {block and block[4:6]}")
    menus = st.get("spaces_menus", {})
    S.check("remote pinned row menu offers Unpin", "Unpin" in menus.get(pinned_id, []), str(menus.get(pinned_id)))
    S.check("remote unpinned tab menu offers Pin", "Pin" in menus.get(f"tab:mlab/{other}", []), str(menus.get(f"tab:mlab/{other}")))
    hosts = [r.split("|")[6] for r in rows if r.startswith("footerHost|")]
    S.check("host footer keeps machines that differ only in case apart", hosts == ["mlab", "MLAB", "Mlab", "pcx"], str(hosts))

    local_pins = lambda: [t["tab_id"] for t in json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"]["tabs"]
                          if t.get("pin_index") is not None]
    def b_pinned(tab):
        return next(t for t in b_snapshot()["tabs"] if t["tab_id"] == tab).get("pin_index") is not None

    def until(pred, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline and not pred():
            time.sleep(0.2)
        return pred()

    S.cmd({"cmd": "spaces_click", "row": pinned_id, "part": "pin"})
    S.check("Unpin on a remote row unpins on that machine", until(lambda: not b_pinned(pin_tab)))
    st = wait(lambda s: not row(s.get("spaces_rows", []), pinned_id), 15)
    S.check("unpinned remote row leaves the pinned section", not row(st.get("spaces_rows", []), pinned_id))
    S.cmd({"cmd": "spaces_click", "row": f"tab:mlab/{other}", "part": "pin"})
    S.check("Pin on a remote row pins on that machine", until(lambda: b_pinned(other)))
    S.check("the local session's pins are untouched", local_pins() == [], str(local_pins()))

    S.app("stop")
    S.lab("down")
    lab_b("down")
    pathlib.Path(S.OUT).write_text("\n".join(S.lines) + "\n")
    if S.failures:
        raise SystemExit("FAIL: " + ", ".join(S.failures))
    print("PASS remote pins")


if __name__ == "__main__":
    main()
