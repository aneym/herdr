#!/usr/bin/env python3
"""P23 check: split hairline, focus cue, divider drag.

  SHELL_LAB=shellspike-x python3 scripts/check_p23.py --out checks/P23.txt
"""
import os
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-x"
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

lines, failures = [], []
OUT = os.path.join(S.D, "checks", "P23.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
CHK = os.path.dirname(OUT)


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


def wait_for(pred, timeout=12):
    t0 = time.time()
    while time.time() - t0 < timeout:
        try:
            v = pred()
        except SystemExit:
            time.sleep(0.2)
            continue
        if v:
            return v
        time.sleep(0.12)
    return None


def herdr_layout(tab):
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    return next(l for l in snap["layouts"] if l["tab_id"] == tab)


def rects(lay):
    return {p["pane_id"]: p["rect"] for p in lay["panes"]}


def parse_rect(text):
    return [float(x) for x in text.replace("{", "").replace("}", "").split(",")]


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


def lin(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def lum(rgb):
    r, g, b = (lin(x) for x in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    l1, l2 = lum(a), lum(b)
    return (max(l1, l2) + 0.05) / (min(l1, l2) + 0.05)


def hairline_contrast(png, st):
    from PIL import Image
    img = Image.open(png).convert("RGB")
    hx, _, hw, hh = parse_rect(st["host_frame"])
    scale = img.width / (hx + hw)
    best = 0.0
    for div in st["dividers"]:
        x, y, w, h = div["frame"]
        if div["vertical"]:
            cx = int((hx + x + w / 2) * scale)
            y0 = int((y + 50) * scale)
            y1 = int((y + h - 20) * scale)
            for col in range(cx - 6, cx + 7):
                ratios = []
                for row in range(max(0, y0), min(img.height, y1), 8):
                    if col < 4 or col + 4 >= img.width:
                        continue
                    mid = img.getpixel((col, row))
                    left = img.getpixel((col - 4, row))
                    right = img.getpixel((col + 4, row))
                    ratios.append(min(contrast(mid, left), contrast(mid, right)))
                if ratios:
                    best = max(best, sorted(ratios)[len(ratios) // 2])
        else:
            cy = int((y + h / 2) * scale)
            x0 = int((hx + x + 20) * scale)
            x1 = int((hx + x + w - 20) * scale)
            for row in range(cy - 6, cy + 7):
                ratios = []
                for col in range(max(0, x0), min(img.width, x1), 8):
                    if row < 4 or row + 4 >= img.height:
                        continue
                    mid = img.getpixel((col, row))
                    above = img.getpixel((col, row - 4))
                    below = img.getpixel((col, row + 4))
                    ratios.append(min(contrast(mid, above), contrast(mid, below)))
                if ratios:
                    best = max(best, sorted(ratios)[len(ratios) // 2])
    return best


def main():
    say(f"HerdrShell P23 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.4)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    tab = next(t["tab_id"] for t in snap["tabs"] if t["label"] == "shell spike")
    say(f"app start: {S.app('start').strip()}")
    S.cmd({"cmd": "select", "tab": tab})
    S.cmd({"cmd": "frame", "w": 1440, "h": 900})

    def two():
        st = S.state()
        if st.get("selected_tab") == tab and len(st.get("dividers", [])) >= 1:
            return st
        return None

    st = wait_for(two, 30)
    check("lab split is on screen", st is not None)
    if st is None:
        return finish()
    S.cmd({"cmd": "split", "direction": "right"})
    st = wait_for(lambda: S.state() if len(S.state().get("dividers", [])) >= 2 else None, 12)
    check("split right added a pane", st is not None)
    if st is None:
        return finish()
    S.cmd({"cmd": "split", "direction": "down"})
    st = wait_for(lambda: (s if len((s := S.state()).get("dividers", [])) >= 3 and len(s.get("surfaces", [])) >= 4 else None), 12)
    lay = herdr_layout(tab)
    check("2x2 split: four panes and three dividers",
          st is not None and len(lay["panes"]) == 4, f"panes={len(lay['panes'])}")
    if st is None:
        return finish()

    focused = []
    for mode, name in (("light", "P23-light.png"), ("dark", "P23-dark.png")):
        S.cmd({"cmd": "appearance", "mode": mode})
        time.sleep(0.5)
        png = shot(name)
        st = S.state()
        ratio = hairline_contrast(png, st) if os.path.getsize(png) > 1000 else 0
        check(f"{mode} hairline contrast is at least 1.3:1 against both neighbours",
              ratio >= 1.3, f"{ratio:.2f}:1 {png}")

    before = S.state()["focused_pane"]
    caps_before = {c["id"]: c["focused"] for c in S.state().get("pane_caps", [])}
    S.key("right", ["cmd", "opt"])
    time.sleep(0.3)
    after = S.state()
    caps_after = {c["id"]: c["focused"] for c in after.get("pane_caps", [])}
    moved = after["focused_pane"] != before and caps_after.get(after["focused_pane"]) is True
    check("focused cue moves with cmd+opt+right", moved,
          f"{before} -> {after.get('focused_pane')} caps {caps_before} -> {caps_after}")
    focused.append(after.get("focused_pane"))

    lay0 = herdr_layout(tab)
    r0 = rects(lay0)
    vertical = next(i for i, d in enumerate(S.state()["dividers"]) if d["vertical"])
    S.cmd({"cmd": "drag_divider", "index": vertical, "dx": 80, "steps": 8, "interval": 0.04})
    time.sleep(0.4)
    wait_for(lambda: not S.state().get("drag_running"), 12)
    lay1 = herdr_layout(tab)
    r1 = rects(lay1)
    changed = any(abs(r1[p]["width"] - r0[p]["width"]) > 5 for p in r0 if p in r1)
    check("divider drag resizes through herdr", changed,
          " ".join(f"{p[-4:]} {r0[p]['width']:.0f}->{r1[p]['width']:.0f}" for p in r0 if p in r1))
    finish()


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        say(f"FAIL {exc}")
        failures.append(str(exc))
        finish()
