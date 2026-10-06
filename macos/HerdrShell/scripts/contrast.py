#!/usr/bin/env python3
"""WCAG contrast check for the P5 theme tokens.

Reads the tokens from the app itself (`HerdrShell --dump-tokens`), so it tests the
values the app ships. For each mode, every text token must reach 4.5:1 against
each surface token it is drawn on (panel and sel), and the terminal foreground
must reach 4.5:1 against the terminal background. With sidebar glass on, the glass text
tokens must reach it on the panel scrim over black and over white, and on `sel`. Exit 1 on any miss.
Usage: contrast.py [--min 4.5] [--tokens tokens.json]
"""
import json
import os
import subprocess
import sys

D = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(D, ".build", "release", "HerdrShell")


def lum(h):
    h = h.lstrip("#")
    c = [int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)]
    c = [x / 12.92 if x <= 0.03928 else ((x + 0.055) / 1.055) ** 2.4 for x in c]
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def ratio(a, b):
    hi, lo = sorted((lum(a), lum(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def blend(fg, backdrop, alpha):
    """Hex of `fg` at `alpha` laid over a gray backdrop level (0-255)."""
    f = [int(fg.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4)]
    return "#%02X%02X%02X" % tuple(round(c * alpha + backdrop * (1 - alpha)) for c in f)


def glass_pairs(t, alpha):
    """Sidebar text with glass on: the panel is a scrim at `alpha` over an unknown desktop,
    so test it over the darkest and lightest desktop; `sel` rows are opaque fills."""
    out = []
    for txt in ("ink", "mute", "orch", "lane", "wf", "ok", "warn"):
        for label, bg in (("panel@black", blend(t["panel"], 0, alpha)), ("panel@white", blend(t["panel"], 255, alpha)),
                          ("sel", t["sel"])):
            out.append((txt, label, bg))
    return out


def main():
    floor = 4.5
    if "--min" in sys.argv:
        floor = float(sys.argv[sys.argv.index("--min") + 1])
    if "--tokens" in sys.argv:
        tok = json.load(open(sys.argv[sys.argv.index("--tokens") + 1]))
    else:
        tok = json.loads(subprocess.run([BIN, "--dump-tokens"], capture_output=True, text=True, check=True).stdout)
    bad = 0
    n = 0
    print(f"contrast floor {floor}:1; terminal theme pair {tok['terminal_theme']}")
    for mode, t in tok["modes"].items():
        pairs = [(txt, surf) for txt in tok["text_tokens"] for surf in tok["surface_tokens"]]
        for txt, surf in pairs:
            r = ratio(t[txt], t[surf])
            ok = r >= floor
            n += 1
            bad += not ok
            print(f"[{'PASS' if ok else 'FAIL'}] {mode:5} {txt:5} {t[txt]} on {surf:5} {t[surf]}  {r:5.2f}:1")
        r = ratio(t["terminalFg"], t["terminalBg"])
        ok = r >= floor
        n += 1
        bad += not ok
        print(f"[{'PASS' if ok else 'FAIL'}] {mode:5} terminalFg {t['terminalFg']} on terminalBg {t['terminalBg']}  {r:5.2f}:1")
    alpha = tok.get("glass_scrim_alpha")
    if alpha is None:
        print("[FAIL] tokens carry no glass_scrim_alpha / glass_modes")
        n += 1
        bad += 1
    else:
        print(f"sidebar glass on: panel scrim alpha {alpha}, glass text tokens")
        for mode, t in tok["glass_modes"].items():
            for txt, label, bg in glass_pairs(t, alpha):
                r = ratio(t[txt], bg)
                ok = r >= floor
                n += 1
                bad += not ok
                print(f"[{'PASS' if ok else 'FAIL'}] glass {mode:5} {txt:5} {t[txt]} on {label:11} {bg}  {r:5.2f}:1")
    print(f"{n - bad}/{n} pairs at or above {floor}:1")
    print("CONTRAST: " + ("PASS" if not bad else "FAIL"))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
