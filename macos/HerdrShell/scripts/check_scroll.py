#!/usr/bin/env python3
"""Trackpad scroll check: a Herdr Shell pane scrolls as many rows per swipe as Ghostty.app,
and a Claude Code pane scrolls one row per wheel report, as Ghostty's own scrollback does.

  python3 scripts/check_scroll.py [--out checks/scroll.txt] [--space] [--attach-bin PATH]
                                  [--claude-accel user|on|off]

Ghostty turns precise trackpad travel into one SGR wheel report per row of travel
(2x the point delta, accumulated against the cell height). The Shell embeds the same
libghostty with the same 2x, so it sends the same reports, and `herdr terminal attach`
scrolls one row per report under HERDR_ATTACH_SCROLL_LINES=1 (ui.mouse_scroll_lines, 3,
otherwise).

Claude Code's fullscreen TUI takes the mouse and adds its own wheel acceleration: it
ramps 0.3 rows per report while reports come under 40 ms apart, up to 6 rows per report,
so a medium swipe scrolled 2.8x and a fast one 4.9x Ghostty's rows, in jumps of up to 6
rows. Alex's ~/.claude/settings.json sets `wheelScrollAccelerationEnabled: false`
(2026-10-06; Claude Code reads it there, not from ~/.claude.json), which gives one row per
report.

Lab `shellspike-scroll`; pane 1 of "shell spike" holds 3000 numbered lines.
1. Host: `herdr terminal attach --no-escape` (HERDR_SHELL_BIN) under a pty, with the
   Shell's attach env and without; 10 wheel-up reports each. Asserts the server's scroll
   offset (`herdr pane get`) moved 1 row per report with the Shell env.
2. Host, Claude Code: the handler ported from Claude Code 2.1.292 runs on Ghostty's
   report timing for each profile, then a real `claude` (throwaway HOME and no account,
   a resumed 3000-row transcript, fullscreen) runs in the lab pane and gets 38 reports 8
   ms apart through the Shell's attach. Both must scroll one row per report (within 15%).
   The setting comes from ~/.claude/settings.json (`user`); `on`/`off` force it, and `on`
   fails, as the head did before the setting.
3. --space: the dev app in the Cua Space, attaching with --attach-bin (default
   HERDR_SHELL_BIN) over the forwarded lab socket. Four trackpad swipes (scroll_gesture
   hook: slow, medium, std, fast; see PROFILES) run; each must scroll Ghostty.app's rows
   for the same swipe within GHOSTTY_TOLERANCE. The std swipe's client-side frame log
   (top visible row per 1/120 s tick) goes next to --out, with its cadence logged.
"""
import json
import os
import pty
import re
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import fcntl
import termios

os.environ["SHELL_LAB"] = "shellspike-scroll"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
SPACE = "--space" in sys.argv
if SPACE:
    os.environ["HERDR_SHELL_SPACE"] = "1"
ATTACH_BIN = (sys.argv[sys.argv.index("--attach-bin") + 1] if "--attach-bin" in sys.argv
              else os.environ["HERDR_SHELL_BIN"])
CLAUDE_ACCEL = (sys.argv[sys.argv.index("--claude-accel") + 1] if "--claude-accel" in sys.argv
                else "user")
D0 = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lab as L  # noqa: E402
import scenario as S  # noqa: E402

if "--out" not in sys.argv:
    S.OUT = os.path.join(D0, "checks", "scroll.txt")
REPORT = b"\x1b[<64;10;10M"  # SGR wheel up at cell 10,10
lines, failures = [], []

# (dy points per event, finger events, momentum events, momentum decay per event), one
# event per 1/120 s, as the scroll_gesture hook and scripts/scroll-ref/inject.swift send them.
PROFILES = {"slow": (2, 60, 0, 0.92), "medium": (6, 30, 40, 0.92),
            "std": (12, 30, 40, 0.92), "fast": (20, 15, 90, 0.95)}
# Rows Ghostty.app 1.3.1 scrolled for each profile: the herdr-qa Space, the Shell's font
# config (SF Mono 13.5, adjust-cell-height 8%), swipes posted to the HID tap by
# scripts/scroll-ref/inject.swift, wheel reports counted by scroll-ref/wheellog.py (one per
# row: Ghostty's scrollback and its reports share one accumulator). Three runs each,
# 2026-10-06: slow 10/10/10, medium 18/18/17, std 38/38/39, fast 54/56/56.
GHOSTTY_ROWS = {"slow": 10, "medium": 18, "std": 38, "fast": 55}
GHOSTTY_TOLERANCE = 0.15


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def pane_info(pane):
    return json.loads(S.lab("herdr", "pane", "get", pane))["result"]["pane"]


def read_frames(fd, seconds):
    end, got = time.time() + seconds, []
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], max(0, end - time.time()))
        if not r:
            continue
        try:
            got.append((time.time(), len(os.read(fd, 65536))))
        except OSError:
            break
    return got


def shell_attach(pane, env_lines):
    """A `herdr terminal attach --no-escape` of the pane under a pty, as the Shell runs it."""
    info = pane_info(pane)
    m, s = pty.openpty()
    fcntl.ioctl(s, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 100, 0, 0))
    env = L.env()
    if env_lines:
        env["HERDR_ATTACH_SCROLL_LINES"] = env_lines
    p = subprocess.Popen([os.environ["HERDR_SHELL_BIN"], "--session", L.SESSION, "terminal", "attach",
                          info["terminal_id"], "--no-escape"],
                         stdin=s, stdout=s, stderr=s, env=env, start_new_session=True)
    os.close(s)
    read_frames(m, 1.5)
    return p, m


def close_attach(p, m):
    p.terminate()
    p.wait(5)
    os.close(m)


def host_attach(pane, env_lines):
    """Rows the server scrolled for 10 wheel reports."""
    p, m = shell_attach(pane, env_lines)
    try:
        start = pane_info(pane)["scroll"]["offset_from_bottom"]
        for _ in range(10):
            os.write(m, REPORT)
            read_frames(m, 0.08)
        time.sleep(0.3)
        moved = pane_info(pane)["scroll"]["offset_from_bottom"] - start
    finally:
        close_attach(p, m)
    return moved


def fill(pane):
    for _ in range(200):
        if "%" in S.pane_read(pane):
            break
        time.sleep(0.05)
    S.lab("herdr", "pane", "run", pane, "seq -f 'line %g' 1 3000")
    S.wait_read(pane, lambda x: "line 3000" in x, 10)


# --- Claude Code ---------------------------------------------------------------------

def claude_rows(times, accel=True, base=1.0):
    """Rows Claude Code 2.1.292's fullscreen TUI scrolls per wheel report at `times` (ms):
    its window-native handler (darwin, not xterm.js, no wheel flood), ported from the
    bundle. Accel ramps the multiplier 0.3 per report under 40 ms apart, up to 6."""
    h = dict(time=0.0, mult=base, dir=0, flip=False, wheel=False, burst=0)
    out = []
    for t in times:
        if h["wheel"] and t - h["time"] > 1500:
            h.update(wheel=False, burst=0, mult=base)
        if h["flip"]:
            h["flip"] = False
            if 1 != h["dir"] or t - h["time"] > 200:
                h.update(dir=1, time=t, mult=base)
                out.append(max(1, int(h["mult"])))
                continue
            h["wheel"] = True
        gap = t - h["time"]
        h.update(dir=1, time=t)
        if h["wheel"]:
            if gap < 5:
                h["burst"] += 1
                if h["burst"] >= 5:
                    h.update(wheel=False, burst=0, mult=base)
                else:
                    out.append(1)
                    continue
            else:
                h["burst"] = 0
        if h["wheel"] and accel:
            decay = 0.5 ** (gap / 150)
            h["mult"] = min(max(15 * min(base, 1), base * 2), 1 + (h["mult"] - 1) * decay + 15 * decay,
                            h["mult"] + 3)
            out.append(max(1, int(h["mult"])))
            continue
        if gap > 40 or not accel:
            h["mult"] = base
        else:
            h["mult"] = min(max(6 * min(base, 1), base * 2), h["mult"] + 0.3)
        out.append(max(1, int(h["mult"])))
    return out


def report_times(dy, steps, momentum, decay, cell=24.0, interval=1000 / 120):
    """When Ghostty sends each wheel report for a profile: 2x the point delta accumulated
    against the cell height, every report of one event at that event's time. 24 px
    reproduces GHOSTTY_ROWS within 2 rows."""
    deltas = [dy] * (steps - 1) + [0] + [dy * decay ** (i + 1) for i in range(momentum)]
    times, pending = [], 0.0
    for i, d in enumerate(deltas):
        pending += 2 * d
        while pending >= cell:
            pending -= cell
            times.append(1000.0 + i * interval)
    return times


def claude_accel_setting():
    if CLAUDE_ACCEL in ("on", "off"):
        return CLAUDE_ACCEL == "on", "forced by --claude-accel"
    path = os.path.expanduser("~/.claude/settings.json")
    try:
        value = json.load(open(path)).get("wheelScrollAccelerationEnabled", True)
    except (OSError, ValueError):
        value = True
    return value is not False, f"{path}: wheelScrollAccelerationEnabled={value}"


def claude_home(home, work, accel):
    """A throwaway Claude Code HOME: no account, onboarding done, fullscreen TUI, the wheel
    setting under test, and one 3000-row transcript to resume. Returns the session id."""
    sid = "5c0f1a2e-0000-4000-8000-000000000001"
    os.makedirs(os.path.join(home, ".claude"), exist_ok=True)
    with open(os.path.join(home, ".claude.json"), "w") as f:
        json.dump({"hasCompletedOnboarding": True, "lastOnboardingVersion": "2.1.292", "theme": "dark",
                   "numStartups": 5, "customApiKeyResponses": {"approved": ["x" * 20], "rejected": []},
                   "projects": {work: {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}}}, f)
    with open(os.path.join(home, ".claude", "settings.json"), "w") as f:
        json.dump({"tui": "fullscreen", "wheelScrollAccelerationEnabled": accel}, f)
    proj = os.path.join(home, ".claude", "projects", re.sub(r"[^A-Za-z0-9]", "-", work))
    os.makedirs(proj, exist_ok=True)
    out, parent = [], None
    for i in range(30):
        for role in ("user", "assistant"):
            n = len(out)
            uid = f"00000000-0000-4000-8000-{n + 1:012d}"
            e = {"parentUuid": parent, "isSidechain": False, "userType": "external", "cwd": work,
                 "sessionId": sid, "version": "2.1.292", "uuid": uid,
                 "timestamp": f"2026-10-06T{10 + n // 60:02d}:{n % 60:02d}:00.000Z"}
            if role == "user":
                e.update(type="user", message={"role": "user", "content": f"question {i}"})
            else:
                text = "\n".join(f"row {i * 100 + k}" for k in range(100))
                e.update(type="assistant", message={
                    "id": f"msg_{n:08d}", "type": "message", "role": "assistant", "model": "claude-sonnet-5-5",
                    "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
                    "stop_sequence": None, "usage": {"input_tokens": 1, "output_tokens": 1}})
            out.append(json.dumps(e))
            parent = uid
    with open(os.path.join(proj, sid + ".jsonl"), "w") as f:
        f.write("\n".join(out) + "\n")
    return sid


def claude_top_row(pane):
    rows = re.findall(r"\brow (\d+)\b", S.lab("herdr", "pane", "read", pane, "--source", "visible"))
    return int(rows[0]) if rows else None


def claude_pane(pane, accel, reports=38):
    """Rows a real Claude Code pane scrolled for `reports` wheel reports 8 ms apart."""
    claude = shutil.which("claude")
    if not claude:
        return None, "no claude on PATH"
    claude = os.path.realpath(claude)
    home = os.path.join(L.LAB, "claude-home")
    shutil.rmtree(home, ignore_errors=True)
    # Outside $HOME: Claude Code reads every ancestor's .claude/ as project settings.
    work = os.path.realpath(tempfile.mkdtemp(prefix="scroll-claude-"))
    sid = claude_home(home, work, accel)
    S.lab("herdr", "pane", "run", pane,
          f"cd {work} && env -i HOME={home} PATH=/usr/bin:/bin TERM=xterm-256color "
          f"ANTHROPIC_API_KEY=sk-ant-check-{'x' * 20} DISABLE_TELEMETRY=1 "
          f"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 {claude} --resume {sid}")
    txt, took = S.wait_read(pane, lambda x: "row 2999" in x, 30)
    if took is None:
        return None, "claude never showed the transcript: " + txt[-300:].replace("\n", " | ")
    time.sleep(1)
    p, m = shell_attach(pane, "1")
    try:
        start = claude_top_row(pane)
        for _ in range(reports):
            os.write(m, REPORT)
            read_frames(m, 0.008)
        read_frames(m, 0.8)
        end = claude_top_row(pane)
    finally:
        close_attach(p, m)
        shutil.rmtree(work, ignore_errors=True)
    if start is None or end is None:
        return None, "no transcript rows on screen"
    return start - end, f"claude {os.path.basename(claude)}"


# --- Space -----------------------------------------------------------------------------

def space_gestures(spike, pane, out_log):
    app_copy = os.path.join(L.LAB, "app", "HerdrShell")
    os.makedirs(os.path.dirname(app_copy), exist_ok=True)
    shutil.copy2(os.path.join(D0, ".build", "release", "HerdrShell"), app_copy + ".new")
    os.replace(app_copy + ".new", app_copy)
    os.environ["HERDR_SHELL_APP"] = app_copy
    os.environ["HERDR_SHELL_ATTACH_BIN"] = os.path.abspath(ATTACH_BIN)  # pushed into the guest
    say(f"app start (Cua Space): {S.app('start', '--herdr', S.guest_path(ATTACH_BIN)).strip()[:160]}")
    S.cmd({"cmd": "select", "tab": spike})
    # Synthesized events reach only a key window; the Space's desktop is the app's own.
    S.cmd({"cmd": "activate"})
    for _ in range(150):
        st = S.state()
        if any(x["pane"] == pane for x in st.get("surfaces", [])):
            break
        time.sleep(0.2)
    time.sleep(1.5)
    moved, log = {}, ""
    for name, (dy, steps, momentum, decay) in PROFILES.items():
        start = pane_info(pane)["scroll"]["offset_from_bottom"]
        out = out_log if name == "std" else os.path.splitext(out_log)[0] + f"-{name}.tsv"
        # S.cmd maps a host "out" into the guest, waits for the file and pulls it back.
        S.cmd({"cmd": "scroll_gesture", "pane": pane, "dy": dy, "steps": steps, "momentum": momentum,
               "decay": decay, "out": out})
        time.sleep(0.3)
        moved[name] = pane_info(pane)["scroll"]["offset_from_bottom"] - start
        if name == "std":
            log = open(out).read()
        elif os.path.exists(out):
            os.unlink(out)
    shot = os.path.splitext(out_log)[0] + ".png"
    S.space("shot", shot)
    S.app("stop")
    return moved, log, shot


def analyse(log):
    head = log.splitlines()[0]
    rows = []
    for l in log.splitlines()[1:]:
        ms, _, top = l.partition("\t")
        n = re.search(r"line (\d+)", top)
        if n:
            rows.append((float(ms), int(n.group(1))))
    changes = [(t, prev - cur) for (_, prev), (t, cur) in zip(rows, rows[1:]) if cur != prev]
    return head, changes


def main():
    say(f"HerdrShell trackpad scroll check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    say(f"attach binary: {ATTACH_BIN}")
    if SPACE:
        S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"]
    spike = next(t["tab_id"] for t in snap["tabs"] if t["label"] == "shell spike")
    lay = next(x for x in snap["layouts"] if x["tab_id"] == spike)
    pane = sorted(lay["panes"], key=lambda p: p["rect"]["x"])[0]["pane_id"]
    fill(pane)

    if not SPACE:
        moved = host_attach(pane, None)
        say(f"default config: 10 wheel reports scrolled {moved} rows")
        check("default attach keeps ui.mouse_scroll_lines (3 rows per report)", moved == 30, f"{moved}")
        moved = host_attach(pane, "1")
        say(f"Shell env (HERDR_ATTACH_SCROLL_LINES=1): 10 wheel reports scrolled {moved} rows")
        check("Shell attach scrolls one row per wheel report", moved == 10, f"{moved}")

        accel, source = claude_accel_setting()
        say(f"Claude Code wheel acceleration: {'on' if accel else 'off'} ({source})")
        for name, prof in PROFILES.items():
            times = report_times(*prof)
            rows = sum(claude_rows(times, accel))
            ok = abs(rows - len(times)) <= max(1, 0.15 * len(times))
            check(f"Claude handler, {name} swipe: one row per report (Ghostty scrollback)", ok,
                  f"{len(times)} reports -> {rows} rows (Ghostty.app {GHOSTTY_ROWS[name]})")
        rows, detail = claude_pane(pane, accel)
        if rows is None:
            check("a Claude Code pane scrolls one row per wheel report", False, detail)
        else:
            check("a Claude Code pane scrolls one row per wheel report", abs(rows - 38) <= 0.15 * 38,
                  f"38 reports 8 ms apart -> {rows} transcript rows ({detail})")
    else:
        out_log = os.path.splitext(S.OUT)[0] + "-frames.tsv"
        moved, log, shot = space_gestures(spike, pane, out_log)
        head, changes = analyse(log)
        say(f"frame log (std swipe): {out_log}  ({head})")
        say(f"screenshot: {shot}")
        for name, want in GHOSTTY_ROWS.items():
            got = moved.get(name, 0)
            check(f"{name} swipe scrolls Ghostty.app's rows", abs(got - want) <= max(2, GHOSTTY_TOLERANCE * want),
                  f"Shell {got}, Ghostty {want}")
        jumps = [d for _, d in changes]
        gaps = [b[0] - a[0] for a, b in zip(changes, changes[1:])]
        if jumps and gaps:
            say(f"std cadence: {len(changes)} visible changes, rows per change {sorted(set(jumps))}, "
                f"ms between changes median {sorted(gaps)[len(gaps) // 2]:.1f} max {max(gaps):.1f}")
        m = re.search(r"events=(\d+) precise=(\d+)", head)
        check("every gesture event reached the surface as precise", bool(m) and m.group(1) == m.group(2), head)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(S.OUT), exist_ok=True)
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
