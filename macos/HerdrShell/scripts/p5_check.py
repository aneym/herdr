#!/usr/bin/env python3
"""P5 check: theme tokens, light/dark, glass. Writes checks/P5.txt (+ P5-light.png, P5-dark.png).

  SHELL_LAB=shellspike-p5 python3 scripts/p5_check.py [--out checks/P5.txt]

1. scripts/contrast.py passes every text token at 4.5:1 in both modes.
2. With the app's override flag `--appearance light`, then `--appearance dark`, an
   in-app screenshot (tagged sRGB) shows the sidebar and the terminal pane at the
   token pair: sidebar = chrome panel token, terminal = the Ghostty theme pair's
   background, and the two modes differ.
3. Live: from dark, the override switches to light with no restart; the screenshot
   follows for chrome and terminal; `system` resolves to the macOS appearance.
4. Glass: off by default on every surface; with the sidebar token on, its text keeps 4.5:1
   over the captured glass background (lightest and darkest desktop); turning the sidebar token on installs the
   glass layer and a translucent window, while the terminal pane stays opaque and at
   its token color.
5. The Ghostty config the surfaces load has only the shell's own keybinds.
Lab session only; nothing touches Alex's live herdr.
"""
import json
import os
import subprocess
import sys
import time

from PIL import Image

D = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NAME = os.environ.get("SHELL_LAB", "shellspike-p5")
os.environ["SHELL_LAB"] = NAME
LAB = os.path.expanduser(f"~/.cache/herdr-build/{NAME}")
STATE = os.path.join(LAB, "state.json")
OUT = os.path.join(D, "checks", "P5.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
CHK = os.path.dirname(OUT)
lines, failures = [], []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def sh(*a):
    r = subprocess.run(a, capture_output=True, text=True)
    return r.stdout + r.stderr if r.returncode else r.stdout


def script(name, *a):
    return sh("python3", os.path.join(D, "scripts", name), *a)


def cmd(obj):
    script("app.py", "cmd", json.dumps(obj))


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def state():
    if os.path.exists(STATE):
        os.unlink(STATE)
    cmd({"cmd": "state", "out": STATE})
    for _ in range(80):
        if os.path.exists(STATE) and os.path.getsize(STATE) > 0:
            time.sleep(0.05)
            return json.load(open(STATE))
        time.sleep(0.05)
    raise SystemExit("no state from app")


def wait_ready():
    for _ in range(300):
        s = state()
        surf = s["surfaces"]
        if len(surf) >= 2 and all(x["visible_nonblank"] for x in surf):
            return s
        time.sleep(0.1)
    return state()


def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def hexs(c):
    return "#%02X%02X%02X" % c[:3]


def sample(img, scale, x, y):
    """Median of a 5x5 block around the point (window points, top-left origin)."""
    px = img.load()
    cx, cy = int(x * scale), int(y * scale)
    vals = [px[min(max(cx + dx, 0), img.width - 1), min(max(cy + dy, 0), img.height - 1)][:3]
            for dx in range(-2, 3) for dy in range(-2, 3)]
    return tuple(sorted(v[i] for v in vals)[len(vals) // 2] for i in range(3))


def sample_rgba(img, scale, x, y):
    """Median RGBA of a 5x5 block (window points, top-left origin)."""
    px = img.load()
    cx, cy = int(x * scale), int(y * scale)
    vals = [px[min(max(cx + dx, 0), img.width - 1), min(max(cy + dy, 0), img.height - 1)]
            for dx in range(-2, 3) for dy in range(-2, 3)]
    return tuple(sorted(v[i] for v in vals)[len(vals) // 2] for i in range(4))


def over(c, backdrop):
    """Composite an RGBA pixel over a gray backdrop level (0-255)."""
    a = c[3] / 255
    return tuple(round(c[i] * a + backdrop * (1 - a)) for i in range(3))


def lum(c):
    v = [x / 255 for x in c[:3]]
    v = [x / 12.92 if x <= 0.03928 else ((x + 0.055) / 1.055) ** 2.4 for x in v]
    return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]


def ratio(a, b):
    hi, lo = sorted((lum(a), lum(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def near(a, b, tol=3):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def shot(path):
    if os.path.exists(path):
        os.unlink(path)
    cmd({"cmd": "shot", "out": path})
    for _ in range(80):
        if os.path.exists(path) and os.path.getsize(path) > 0:
            time.sleep(0.1)
            return Image.open(path).convert("RGBA")
        time.sleep(0.1)
    raise SystemExit("no screenshot")


def points(s, img):
    """Sample points from the state: blank sidebar area and the bottom-right of the host."""
    wf = [float(v) for v in s["window_frame"].replace("{", "").replace("}", "").split(",")]
    ww, wh = wf[2], wf[3]
    scale = img.width / ww
    hf = [float(v) for v in s["theme"]["host_frame"].replace("{", "").replace("}", "").split(",")]
    # Blank part of the sidebar's DETAIL block (right of the short text lines), which a
    # window-server capture and an in-process layer render both draw.
    sidebar_pt = (285, wh - 75)
    term_pt = (hf[0] + hf[2] - 40, wh - 40)
    return scale, sidebar_pt, term_pt


def measure(label, png):
    s = wait_ready()
    s = state()
    img = shot(png)
    scale, sp, tp = points(s, img)
    return s, sample(img, scale, *sp), sample(img, scale, *tp)


def start_app(*extra):
    script("app.py", "stop")
    time.sleep(0.5)
    say(f"app start {' '.join(extra)}: {script('app.py', 'start', *extra).strip()}")


def main():
    say(f"HerdrShell P5 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}  lab={NAME}")
    say(f"host load average: {os.getloadavg()[0]:.0f} on {os.cpu_count()} cores")

    # 1. contrast
    r = subprocess.run([sys.executable, os.path.join(D, "scripts", "contrast.py")], capture_output=True, text=True)
    for l in r.stdout.splitlines():
        say("  " + l)
    check("contrast: every text token at 4.5:1 in light and dark", r.returncode == 0)
    tok = json.loads(subprocess.run([os.path.join(D, ".build/release/HerdrShell"), "--dump-tokens"],
                                    capture_output=True, text=True).stdout)["modes"]

    script("lab.py", "down")
    time.sleep(0.5)
    up = script("lab.py", "up").strip().splitlines()
    say(f"lab up: {up[-1] if up else ''}")

    seen = {}
    # 2. override flag, light then dark
    for mode in ("light", "dark"):
        start_app("--appearance", mode)
        png = os.path.join(CHK, f"P5-{mode}.png")
        s, side, term = measure(mode, png)
        th = s["theme"]
        t = tok[mode]
        say(f"--- --appearance {mode}: effective={th['effective']} system={th['system']} "
            f"window_appearance={th['window_appearance']} terminal theme pair={th['terminal_theme']}")
        say(f"    sidebar pixel {hexs(side)} (token panel {t['panel']}); terminal pixel {hexs(term)} (token terminalBg {t['terminalBg']})")
        check(f"{mode}: override flag sets the effective mode", th["override"] == mode and th["effective"] == mode)
        check(f"{mode}: window uses the {mode} native appearance",
              ("Dark" in th["window_appearance"]) == (mode == "dark"), th["window_appearance"])
        check(f"{mode}: sidebar background matches the chrome token", near(side, rgb(t["panel"])))
        check(f"{mode}: terminal background matches the Ghostty theme pair", near(term, rgb(t["terminalBg"])))
        seen[mode] = (side, term)
        if mode == "light":
            script("app.py", "stop")
    check("light and dark differ in both chrome and terminal",
          not near(seen["light"][0], seen["dark"][0], 20) and not near(seen["light"][1], seen["dark"][1], 20),
          f"chrome {hexs(seen['light'][0])} vs {hexs(seen['dark'][0])}; terminal {hexs(seen['light'][1])} vs {hexs(seen['dark'][1])}")

    # dark app is still running: 3. live switch
    s = state()
    th = s["theme"]
    check("glass tokens default off (sidebar, overlay); terminal panes opaque",
          th["glass"] == {"sidebar": False, "overlay": False} and th["sidebar_glass_view"] == "none"
          and th["window_opaque"] and th["surfaces_opaque"], json.dumps(th["glass"]))
    own = [l.strip() for l in open(os.path.join(D, "Resources", "ghostty.conf")) if l.strip().startswith("keybind")]
    check("ghostty config carries only the shell's own keybinds (Alex's are stripped)",
          th["config_keybinds"] == own, f"{len(own)} shell keybinds, loaded {len(th['config_keybinds'])}")

    cmd({"cmd": "appearance", "mode": "light"})
    time.sleep(0.8)
    s, side, term = measure("live-light", os.path.join(CHK, "P5-live-light.png"))
    th = s["theme"]
    say(f"--- live switch dark -> light: sidebar {hexs(side)}, terminal {hexs(term)}")
    check("live: override dark -> light without restart flips chrome and terminal together",
          th["effective"] == "light" and near(side, rgb(tok["light"]["panel"])) and near(term, rgb(tok["light"]["terminalBg"])))

    cmd({"cmd": "appearance", "mode": "system"})
    time.sleep(0.8)
    th = state()["theme"]
    check("live: 'system' follows the macOS appearance", th["override"] == "system" and th["effective"] == th["system"],
          f"system={th['system']}")

    # 4. glass
    cmd({"cmd": "appearance", "mode": "dark"})
    cmd({"cmd": "glass", "surface": "sidebar", "on": True})
    time.sleep(0.8)
    s, side, term = measure("glass", os.path.join(CHK, "P5-glass.png"))
    th = s["theme"]
    say(f"--- sidebar glass on: view={th['sidebar_glass_view']} window_opaque={th['window_opaque']}; "
        f"sidebar {hexs(side)}, terminal {hexs(term)}")
    check("glass: sidebar token installs the glass layer and a translucent window",
          th["glass"]["sidebar"] and th["sidebar_glass_view"] != "none" and not th["window_opaque"])
    check("glass: terminal pane stays opaque at its token color",
          th["surfaces_opaque"] and near(term, rgb(tok["dark"]["terminalBg"])), hexs(term))
    # The sidebar text must stay legible over the glass. The desktop behind is unknown, so
    # take the sidebar pixel the screenshot really has (RGBA), lay it over the darkest and
    # the lightest desktop, and test every glass text token on both, plus on `sel`.
    gimg = Image.open(os.path.join(CHK, "P5-glass.png")).convert("RGBA")
    gscale, gsp, _ = points(s, gimg)
    raw = sample_rgba(gimg, gscale, *gsp)
    gt = json.loads(subprocess.run([os.path.join(D, ".build/release/HerdrShell"), "--dump-tokens"],
                                   capture_output=True, text=True).stdout)["glass_modes"]["dark"]
    bgs = {"panel@black": over(raw, 0), "panel@white": over(raw, 255), "sel": rgb(gt["sel"])}
    say(f"    sidebar pixel RGBA {raw} -> over black {hexs(bgs['panel@black'])}, over white {hexs(bgs['panel@white'])}")
    worst = min((ratio(rgb(gt[k]), bg), k, name) for k in ("ink", "mute", "orch", "lane", "wf", "ok", "warn")
                for name, bg in bgs.items())
    for k in ("ink", "mute", "orch", "lane", "wf", "ok", "warn"):
        say("    " + "  ".join(f"{k} on {name} {ratio(rgb(gt[k]), bg):.2f}:1" for name, bg in bgs.items()))
    check("glass: sidebar text at 4.5:1 over the captured glass background, on the darkest and lightest desktop",
          worst[0] >= 4.5, f"worst {worst[0]:.2f}:1 ({worst[1]} on {worst[2]})")
    alpha = th["glass_scrim_alpha"]
    pan = rgb(tok["dark"]["panel"])
    lo = tuple(round(c * alpha) for c in pan)
    hi = tuple(round(c * alpha + 255 * (1 - alpha)) for c in pan)
    # A capture with real glass mixes the panel scrim with the desktop: every channel sits
    # between the scrim over black and the scrim over white. Bare white glass (no scrim)
    # is outside that range, and so is an unscrimmed dark one.
    inside = all(lo[i] - 3 <= raw[i] <= hi[i] + 3 for i in range(3))
    check("glass: sidebar carries the panel scrim (captured pixel between scrim-over-black and scrim-over-white)",
          0.5 < alpha < 1 and inside, f"alpha {alpha}, pixel {hexs(raw)}, range {hexs(lo)}..{hexs(hi)}")
    cmd({"cmd": "glass", "surface": "sidebar", "on": False})
    time.sleep(0.8)
    s, side, term = measure("glass-off", os.path.join(CHK, "P5-glass-off.png"))
    check("glass: token off restores the opaque panel",
          s["theme"]["sidebar_glass_view"] == "none" and s["theme"]["window_opaque"] and near(side, rgb(tok["dark"]["panel"])),
          hexs(side))

    script("app.py", "stop")
    time.sleep(0.5)
    say(f"lab down: {script('lab.py', 'down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
