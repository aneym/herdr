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
from datetime import datetime, timezone

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
        f.write("\n".join(ln.replace(os.path.expanduser("~"), "~") for ln in lines) + "\n")
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


def dist(a, b):
    return abs(a[0] - b[0]) + abs(a[1] - b[1]) + abs(a[2] - b[2])


def cap_band(png, st):
    """The cap's label sits inside its 36 pt band, and the terminal row under it is not cap fill."""
    from PIL import Image
    img = Image.open(png).convert("RGB")
    content = st.get("content_size") or [img.width, img.height]
    scale = img.width / content[0] if content[0] else 1
    problems = []
    for cap in st.get("pane_caps") or []:
        win = cap.get("window") or []
        if len(win) != 4 or win[2] < 20 or win[3] < 20:
            continue
        x, y, w, h = win

        def px(xx, yy):
            return img.getpixel((min(img.width - 1, max(0, int(xx))), min(img.height - 1, max(0, int(yy)))))

        y_mid = (y + h / 2) * scale
        x0, x1 = (x + 24) * scale, (x + min(180, w * 0.45)) * scale
        fill = px((x + w / 2) * scale, y_mid)
        ink = 0
        for yy in range(int((y + 6) * scale), int((y + h - 6) * scale), 2):
            for xx in range(int(x0), int(x1), 3):
                if dist(px(xx, yy), fill) > 36:
                    ink += 1
        if ink < 8:
            problems.append(f"{cap.get('name')} label not inside the cap")
        below = []
        yb0 = int((y + h + 3) * scale)
        yb1 = int((y + h + 12) * scale)
        for yy in range(yb0, max(yb0 + 1, yb1)):
            for xx in range(int((x + 12) * scale), int((x + w - 12) * scale), 8):
                below.append(px(xx, yy))
        if below and sum(1 for p in below if dist(p, fill) < 18) > len(below) * 0.85:
            problems.append(f"{cap.get('name')} overpaints the first terminal row")
    return not problems, "; ".join(problems)


def text_rows(png, left, top, right, bottom):
    from PIL import Image
    img = Image.open(png).convert("RGB")
    hits = 0
    y0, y1 = max(0, int(top)), min(img.height, int(bottom))
    x0, x1 = max(0, int(left)), min(img.width, int(right))
    for y in range(y0, y1, 4):
        lums = []
        for x in range(x0, x1, 6):
            r, g, b = img.getpixel((x, y))
            lums.append(0.2126 * r + 0.7152 * g + 0.0722 * b)
        if lums and max(lums) - min(lums) > 28:
            hits += 1
    return hits


def title_variety(png):
    from PIL import Image
    img = Image.open(png).convert("RGB")
    band = max(40, img.height // 18)
    colors = set()
    for y in range(2, band, 2):
        for x in range(img.width - int(img.width * 0.18), img.width - 8, 3):
            colors.add(img.getpixel((x, y)))
    return len(colors)


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
            "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "hosts": [
                {"name": "host-1", "summary": "lab", "usage": {
                    "state": "ok", "load_per_core": 1.25, "cpu_pct": 40,
                    "mem_used_mb": 16000, "mem_total_mb": 64000,
                    "slots_used": 2, "slots_total": 8, "age_s": 4}},
                {"name": "host-2", "summary": "old", "usage": {
                    "state": "idle", "load_per_core": 0.1, "cpu_pct": 1,
                    "mem_used_mb": 100, "mem_total_mb": 1000,
                    "slots_used": 0, "slots_total": 1, "age_s": 90}},
            ],
        }, f)
    os.environ["FACTORY_OVERLAY"] = overlay
    os.environ["HERDR_SHELL_COMMIT"] = "p22fixture01"
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
    st = S.state()
    band_ok, band_detail = cap_band(light, st)
    check("cap label sits inside the band and the terminal's first row is untouched",
          st.get("update_pill") is False and band_ok, band_detail or "pill hidden")
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
                    and any(m.get("name") == "host-1" and "1.25/core" in m.get("usage", "") and "40%" in m.get("usage", "")
                             and "16000/64000 MB" in m.get("usage", "") and "2/8" in m.get("usage", "")
                             and m.get("usage_state") == "ok" for m in s.get("machines", [])), 15)
    stale = None if st is None else next((m for m in st.get("machines", []) if m.get("name") == "host-2"), None)
    check("Factory row opens the Factory view and a machine row shows fresh usage",
          st is not None, "" if st is None else str(st.get("machines")))
    check("usage older than 30s is hidden", stale is not None and stale.get("usage") == "", str(stale))
    S.cmd({"cmd": "appearance", "mode": "light"})
    time.sleep(0.3)
    fl = shot("P22-factory-light.png")
    S.cmd({"cmd": "appearance", "mode": "dark"})
    time.sleep(0.3)
    fd = shot("P22-factory-dark.png")
    fst = S.state()
    content = fst.get("content_size") or [1440, 900]
    host = fst.get("host_frame") or ""
    rows = text_rows(fl, content[0] * 0.28, 40, content[0] - 20, content[1] - 40)
    names = [m.get("name") for m in fst.get("machines") or []]
    check("light and dark screenshots show the Factory view, machine rows and usage",
          fst.get("shell", {}).get("factory_open") is True
          and "host-1" in names and any("1.25/core" in (m.get("usage") or "") for m in fst.get("machines") or [])
          and rows >= 6 and os.path.getsize(fd) > 1000,
          f"rows={rows} names={names} host={host}")

    running = "" if st is None else str(st.get("shell", {}).get("running_commit") or "")
    check("running commit is the injected non-empty value", running == "p22fixture01", f"running={running!r}")
    quiet = shot("P22-pill-off.png")
    off_colors = title_variety(quiet)
    write_staged("p22-other-commit")
    S.cmd({"cmd": "updates"})
    shown = wait_state(lambda s: s.get("update_pill") is True, 8)
    pill = shot("P22-pill.png")
    on_colors = title_variety(pill)
    check("update pill shows when staged.json has a different commit",
          shown is not None and on_colors > off_colors + 2, f"running={running!r} title colors {off_colors}->{on_colors}")
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
