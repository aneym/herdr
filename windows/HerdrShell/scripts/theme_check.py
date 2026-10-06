#!/usr/bin/env python3
"""Light/dark check for the running Windows Herdr Shell, driven from Studio.

For each mode it sets the app's own appearance override through the control
pipe (never the Windows theme), takes a window shot, and checks that the title
bar, the sidebar and the pane area all switched: median luminance above 200 in
light, below 80 in dark. The override is reset to `system` at the end, even on
failure. Shots are kept in --out-dir for review.

    python3 windows/HerdrShell/scripts/theme_check.py --out-dir /tmp/theme
"""

import argparse
import importlib.util
import json
import struct
import sys
import time
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("pc", HERE / "pc.py")
pc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pc)


def read_png(path):
    """(width, height, rows of RGB tuples) for an 8-bit RGB or RGBA non-interlaced PNG."""
    data = Path(path).read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    pos, idat = 8, b""
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        if kind == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or color not in (2, 6) or interlace:
                raise ValueError(f"{path}: unsupported PNG (depth {depth}, color {color}, interlace {interlace})")
            bpp = 3 if color == 2 else 4
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw, stride, rows, prev = zlib.decompress(idat), w * bpp, [], bytearray(w * bpp)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b, c = prev[i], prev[i - bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc_ = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc_ else b if pb <= pc_ else c)) & 255
        rows.append([tuple(line[x * bpp:x * bpp + 3]) for x in range(w)])
        prev = line
    return w, h, rows


def median_luma(rows, x0, y0, x1, y1):
    vals = sorted(int(0.2126 * r + 0.7152 * g + 0.0722 * b) for row in rows[y0:y1:3] for (r, g, b) in row[x0:x1:3])
    return vals[len(vals) // 2]


def regions(w, h):
    """Title bar strip, sidebar column, pane area, as fractions of the window shot."""
    return {
        "title_bar": (int(w * 0.30), 4, int(w * 0.70), 24),
        "sidebar": (int(w * 0.01), int(h * 0.10), int(w * 0.12), int(h * 0.95)),
        "pane": (int(w * 0.30), int(h * 0.10), int(w * 0.95), int(h * 0.95)),
    }


def ctl(obj):
    rc, out = pc.ctl_send(obj, timeout=30)
    try:
        return json.loads(out) if rc == 0 else {"ok": False, "error": out or f"rc {rc}"}
    except json.JSONDecodeError:
        return {"ok": False, "error": out}


def check_mode(mode, out_dir):
    failures = []
    reply = ctl({"cmd": "appearance", "mode": mode})
    if not reply.get("ok") or reply.get("mode") != mode:
        return [f"{mode}: appearance override failed: {reply}"], {}
    time.sleep(0.8)
    ui = ctl({"cmd": "ui"})
    if (ui.get("appearance") or {}).get("mode") != mode:
        failures.append(f"{mode}: ui reports appearance {ui.get('appearance')}")
    rpath = f"{pc.R_SHOTS}/theme-{mode}-{time.strftime('%Y%m%d-%H%M%S')}.png".replace("/", "\\")
    shot = ctl({"cmd": "shot", "out": rpath})
    local = Path(out_dir) / f"after-{mode}.png"
    if not shot.get("ok") or pc.scp_from(rpath.replace("\\", "/"), local) != 0:
        return failures + [f"{mode}: shot failed: {shot}"], {}
    w, h, rows = read_png(local)
    lumas = {name: median_luma(rows, *box) for name, box in regions(w, h).items()}
    for name, luma in lumas.items():
        if (mode == "light" and luma <= 200) or (mode == "dark" and luma >= 80):
            failures.append(f"{mode}: {name} median luminance {luma}")
    return failures, lumas


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-dir", required=True)
    args = ap.parse_args()
    Path(args.out_dir).mkdir(parents=True, exist_ok=True)
    pc.bootstrap()
    game, _ = pc.guard(quiet=True)
    if game:
        print("game running; not touching the app window", file=sys.stderr)
        return 75
    failures, report = [], {}
    try:
        for mode in ("light", "dark"):
            f, lumas = check_mode(mode, args.out_dir)
            failures += f
            report[mode] = lumas
    finally:
        reset = ctl({"cmd": "appearance", "mode": "system"})
        report["reset"] = reset
    print(json.dumps(report, indent=1))
    for f in failures:
        print(f"FAIL {f}")
    print("PASS" if not failures else f"{len(failures)} failure(s)")
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
