#!/usr/bin/env python3
"""P22 check: one window. Factory usage, per-pane chat, the update pill.

  SHELL_LAB=shellspike-w python3 scripts/check_p22.py --out checks/P22.txt

The lab session is the only herdr this talks to. The app is started with
--agent-run (app.py adds it) and stays offscreen.
"""
import json
import os
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-w"
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
OUT = os.path.join(S.D, "checks", "P22.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
CHK = os.path.dirname(OUT)
SUPPORT = os.path.join(S.LAB, "h", "Library", "Application Support", "HerdrShell Dev")


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def finish():
    S.check_front(check)
    S.app("stop")
    time.sleep(0.4)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


def wait_state(pred, timeout=20):
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
        time.sleep(0.15)
    return None


def caps(st):
    return {c["id"]: c for c in st.get("pane_caps", [])}


def shot(name):
    png = os.path.join(CHK, name)
    if os.path.exists(png):
        os.unlink(png)
    S.cmd({"cmd": "shot", "out": png})
    for _ in range(40):
        if os.path.exists(png) and os.path.getsize(png) > 1000:
            return png
        time.sleep(0.1)
    return png


def write_staged(commit):
    os.makedirs(SUPPORT, exist_ok=True)
    with open(os.path.join(SUPPORT, "staged.json"), "w") as f:
        json.dump({"commit": commit, "built_at": "2026-10-02T16:00:00Z", "ref": "p22", "notes": []}, f)
        f.write("\n")


def main():
    say(f"HerdrShell P22 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.4)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    tabs = {t["tab_id"]: t["label"] for t in snap["tabs"]}
    tab = next(k for k, v in tabs.items() if v == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == tab)
    p1, p2 = [p["pane_id"] for p in sorted(lay["panes"], key=lambda p: p["rect"]["x"])]
    say(f"split panes: {p1} {p2}")
    for _ in range(80):
        if "%" in S.lab("herdr", "pane", "read", p1, "--source", "visible"):
            break
        time.sleep(0.05)
    S.lab("herdr", "pane", "report-agent", p1, "--source", "spike", "--agent", "claude", "--state", "idle")

    overlay = os.path.join(S.LAB, "overlay.json")
    with open(overlay, "w") as f:
        json.dump({
            "generated_at": "2026-10-02T16:00:00Z",
            "hosts": [
                {"name": "studio", "summary": "lab", "usage": {
                    "state": "ok", "load_per_core": 1.25, "cpu_pct": 40,
                    "mem_used_mb": 16000, "mem_total_mb": 64000,
                    "slots_used": 2, "slots_total": 8, "age_s": 4}},
                {"name": "stale", "summary": "old", "usage": {
                    "state": "idle", "load_per_core": 0.1, "cpu_pct": 1,
                    "mem_used_mb": 100, "mem_total_mb": 1000,
                    "slots_used": 0, "slots_total": 1, "age_s": 90}},
            ],
        }, f)
    os.environ["FACTORY_OVERLAY"] = overlay
    say(f"app start: {S.app('start').strip()}")
    S.cmd({"cmd": "select", "tab": tab})
    S.cmd({"cmd": "frame", "w": 1440, "h": 900})
    st = wait_state(lambda s: p1 in caps(s) and p2 in caps(s) and caps(s)[p1]["agent"] is True, 30)
    check("agent pane and blank pane both have caps", st is not None)
    if st is None:
        return finish()
    c = caps(st)
    check("blank terminal has no toggle", c[p2]["agent"] is False and c[p2]["chat"] is False, str(c[p2]))
    check("agent pane is named from herdr and starts as a terminal",
          c[p1]["name"] == "claude" and c[p1]["chat"] is False and c[p1]["agent"] is True)

    S.cmd({"cmd": "pane_mode", "id": p1, "mode": "chat"})
    st = wait_state(lambda s: caps(s).get(p1, {}).get("chat") is True and caps(s).get(p2, {}).get("chat") is False, 8)
    check("pane A is chat while pane B stays a terminal", st is not None)
    S.cmd({"cmd": "appearance", "mode": "light"})
    time.sleep(0.4)
    light = shot("P22-light.png")
    S.cmd({"cmd": "appearance", "mode": "dark"})
    time.sleep(0.4)
    dark = shot("P22-dark.png")
    check("light and dark screenshots of the split with chat",
          os.path.getsize(light) > 1000 and os.path.getsize(dark) > 1000, f"{light} {dark}")

    S.app("stop")
    time.sleep(0.6)
    say(f"relaunch: {S.app('start').strip()}")
    S.cmd({"cmd": "select", "tab": tab})
    st = wait_state(lambda s: caps(s).get(p1, {}).get("chat") is True and caps(s).get(p2, {}).get("chat") is False
                    and caps(s).get(p2, {}).get("agent") is False, 30)
    check("chat mode persists across relaunch and the blank pane still has no toggle", st is not None,
          "" if st is None else str({k: caps(st)[k] for k in (p1, p2)}))

    S.cmd({"cmd": "factory", "open": True})
    st = wait_state(lambda s: s.get("shell", {}).get("factory_open") is True
                    and any(m.get("name") == "studio" and "1.25/core" in m.get("usage", "") and "40%" in m.get("usage", "")
                             and "16000/64000 MB" in m.get("usage", "") and "2/8" in m.get("usage", "")
                             and m.get("usage_state") == "ok" for m in s.get("machines", [])), 15)
    stale = None if st is None else next((m for m in st.get("machines", []) if m.get("name") == "stale"), None)
    check("Factory row opens the Factory view and a machine row shows fresh usage",
          st is not None, "" if st is None else str(st.get("machines")))
    check("usage older than 30s is hidden", stale is not None and stale.get("usage") == "", str(stale))
    S.cmd({"cmd": "appearance", "mode": "light"})
    time.sleep(0.3)
    fl = shot("P22-factory-light.png")
    S.cmd({"cmd": "appearance", "mode": "dark"})
    time.sleep(0.3)
    fd = shot("P22-factory-dark.png")
    check("light and dark screenshots of the Factory view and sidebar",
          os.path.getsize(fl) > 1000 and os.path.getsize(fd) > 1000, f"{fl} {fd}")

    running = "" if st is None else str(st.get("shell", {}).get("running_commit") or "")
    write_staged("p22-other-commit")
    S.cmd({"cmd": "updates"})
    shown = wait_state(lambda s: s.get("update_pill") is True, 8)
    check("update pill shows when staged.json has a different commit", shown is not None, f"running={running!r}")
    write_staged(running)
    S.cmd({"cmd": "updates"})
    hidden = wait_state(lambda s: s.get("update_pill") is False, 8)
    check("update pill is absent when the staged commit matches", hidden is not None, f"running={running!r}")

    finish()


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        say(f"FAIL {exc}")
        failures.append(str(exc))
        finish()
