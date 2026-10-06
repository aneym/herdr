#!/usr/bin/env python3
"""Cross-machine image paste check: Cmd-V of an image-only clipboard in Herdr Shell
reaches a pane on another machine as a path on that machine.

  python3 scripts/check_clipboard_paste.py [--out checks/clipboard-paste.txt]

Lab `shellspike-paste`. The app runs in the Cua Space (its own macOS guest) and
attaches to a Studio lab server over a forwarded socket, the same shape as Book's
Shell on Studio's server. Pane 1 runs a stand-in agent that reads one line and
reports whether that path is a PNG on its own host. The image goes onto a private
named pasteboard in the guest (never the user's clipboard); Cmd-V goes through the
Edit menu exactly as a keypress. Asserts from `herdr pane read`: the agent got a
path, the file is a PNG on Studio, and the guest has no such file.
"""
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import time
import zlib

os.environ["SHELL_LAB"] = "shellspike-paste"
os.environ.setdefault("HERDR_SHELL_SPACE", "1")
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LABDIR = os.path.expanduser(f"~/.cache/herdr-build/{os.environ['SHELL_LAB']}")
os.makedirs(os.path.join(LABDIR, "app"), exist_ok=True)
APP_COPY = os.path.join(LABDIR, "app", "HerdrShell")
os.environ["HERDR_SHELL_APP"] = APP_COPY
PNG = os.path.join(LABDIR, "clip.png")
# Any HERDR_SHELL_* absolute path is pushed into the guest at start.
os.environ["HERDR_SHELL_CHECK_PNG"] = PNG
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402

if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "clipboard-paste.txt")
lines, failures = [], []

AGENT = r'''while IFS= read -r p; do
  if [ -f "$p" ]; then echo "AGENT-GOT $(hostname -s) $(file -b "$p" | cut -d, -f1-2) $p";
  else echo "AGENT-MISSING $p"; fi
done'''


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def herdr(*a):
    return S.lab("herdr", *a)


def write_png(path, w=3, h=2):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    raw = b"".join(b"\x00" + b"\x20\x60\xe0\xff" * w for _ in range(h))
    data = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(data)


def wait_state(pred, timeout=25):
    t0, last = time.time(), None
    while time.time() - t0 < timeout:
        try:
            last = S.state()
        except (SystemExit, RuntimeError):
            time.sleep(0.2)
            continue
        if pred(last):
            return last
        time.sleep(0.2)
    return last


def main():
    say(f"HerdrShell cross-machine image paste check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    write_png(PNG)
    snap = json.loads(herdr("api", "snapshot"))["result"]["snapshot"]
    tabs = {x["tab_id"]: x["label"] for x in snap["tabs"]}
    spike = next(k for k, v in tabs.items() if v == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == spike)
    p1 = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
    for _ in range(200):
        if "%" in S.pane_read(p1):
            break
        time.sleep(0.05)
    herdr("pane", "run", p1, AGENT)
    time.sleep(0.5)

    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), APP_COPY + ".new")
    os.replace(APP_COPY + ".new", APP_COPY)
    say(f"app start (Cua Space guest, lab socket forwarded from Studio): {S.app('start').strip()[:160]}")
    S.cmd({"cmd": "select", "tab": spike})
    s = wait_state(lambda s: s.get("selected_tab") == spike and s.get("focused_pane") == p1
                   and any(x["pane"] == p1 for x in s.get("surfaces", [])), 60)
    check("guest Shell shows the lab pane", s is not None and s.get("focused_pane") == p1,
          f"focused={None if s is None else s.get('focused_pane')}")

    S.cmd({"cmd": "clipboard_image", "path": S.guest_path(PNG)})
    time.sleep(0.3)
    S.key("v", ["cmd"])
    # The paste lands on the agent's input line before Return (async upload first).
    txt, dt = S.wait_read(p1, lambda x: "herdr-clipboard-images-" in x, 15)
    check("Cmd-V put a server-staged path on the agent's input line", dt is not None,
          f"{dt:.2f}s after Cmd-V" if dt is not None else txt[-200:])
    # Ghostty pastes the path without a newline; Return hands it to the agent.
    S.key("return")
    # The pane is narrow: join wrapped rows before matching the agent's report line.
    agent_re = re.compile(r"AGENT-(GOT (\S+) (PNG image data, \d+ x \d+)|MISSING) ?(/\S+?\.png)?")
    txt, dt = S.wait_read(p1, lambda x: agent_re.search("".join(x.splitlines()).split("done", 1)[-1]), 15)
    m = agent_re.search("".join(txt.splitlines()).split("done", 1)[-1])
    say(f"herdr pane read {p1} --source recent (agent pane on Studio):")
    for l in [l for l in txt.splitlines() if l.strip()][-4:]:
        say(f"  | {l}")
    host = subprocess.run(["hostname", "-s"], capture_output=True, text=True).stdout.strip()
    check("agent on Studio received a path to a PNG on its own host",
          bool(m) and m.group(2) == host and m.group(3) == "PNG image data, 3 x 2", m.group(0) if m else "no agent line")
    path = m.group(4) if m else None
    check("the path is in the server's staging dir", bool(path) and "/herdr-clipboard-images-" in path, str(path))
    if path:
        guest = S.space("exec", f"test -e {path} && echo present || echo absent").strip()
        check("the client machine (guest) has no such file", guest.endswith("absent"), guest)
    finish()


def finish():
    S.app("stop")
    time.sleep(0.4)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(S.OUT), exist_ok=True)
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
