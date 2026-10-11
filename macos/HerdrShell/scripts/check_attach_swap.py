#!/usr/bin/env python3
"""Machine-swap check: a pane another machine still holds opens here without a banner.

  python3 scripts/check_attach_swap.py [--no-build] [--app BIN] [--out checks/ATTACH-SWAP.txt] [--new-threads]

--new-threads runs only cases 3 and 4 (new threads) on a fresh lab.

--app runs another build (for example origin/main's, to see the check fail there).

Alex, 2026-10-10: "swapping between machines seems to get it stuck". The app runs in the
Cua Space (HERDR_SHELL_SPACE=1), reaching the lab server over the Space's socket bridge,
as a second machine reaches Studio over its tunnel. The first machine is a CLI
`herdr terminal attach` on the host.

  1. The host client attaches to a pane, then stops (SIGSTOP): a machine gone to sleep
     with its socket still open. The app starts: the pane attaches with no notice and no
     raw `]4;N;rgb:` color replies on screen, typing reaches it, and the stopped client
     is shut out when it wakes.
  2. The host takes the pane back with --takeover: the app shows it held. Keys typed in the
     app take it back and land (auto take-control). The host takes it again; a click on the
     pane takes it back: typing works afterwards and the host client is shut out.
  3. The app opens a new thread (split) while the host attaches every new pane: keys typed
     at once land in it with no click, and the host client is refused or shut out.
  4. The host opens a new thread in the tab the app shows and attaches it: the app shows it
     held and does not take it; the host's keys land.
"""
import json
import os
import pty
import select
import signal
import subprocess
import sys
import threading
import time

os.environ["SHELL_LAB"] = "shellspike-swap"
os.environ["HERDR_SHELL_SPACE"] = "1"
# Lab server and the Space's attach client run the same installed herdr.
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lab as L  # noqa: E402
import scenario as S  # noqa: E402

D = S.D
SCRATCH = os.path.expanduser("~/.cache/herdr-build/swap-swift")
APP_BIN = os.path.join(SCRATCH, "release", "HerdrShell")
if "--app" in sys.argv:
    APP_BIN = os.path.abspath(sys.argv[sys.argv.index("--app") + 1])
LAB_BIN = L.BIN
OUT = os.path.join(D, "checks", "ATTACH-SWAP.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
lines, failures = [], []
clients = []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def wait(pred, timeout, step=0.1):
    t0 = time.time()
    while time.time() - t0 < timeout:
        v = pred()
        if v:
            return v
        time.sleep(step)
    return None


class Cli:
    """`herdr terminal attach` in its own pty: the other machine's client."""

    def __init__(self, terminal, takeover):
        env = {l.split("=", 1)[0]: l.split("=", 1)[1] for l in S.lab("env").splitlines() if "=" in l}
        self.master, slave = pty.openpty()
        subprocess.run(["stty", "rows", "30", "cols", "100"], stdin=slave)
        argv = [LAB_BIN, "--session", S.NAME, "terminal", "attach", terminal, "--no-escape"]
        self.proc = subprocess.Popen(argv + (["--takeover"] if takeover else []),
                                     stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        self.out = b""
        clients.append(self)

    def drain(self, sec):
        end = time.time() + sec
        while time.time() < end:
            r, _, _ = select.select([self.master], [], [], 0.05)
            if r:
                try:
                    self.out += os.read(self.master, 65536)
                except OSError:
                    break
        return self.out

    def close(self):
        if self.proc.poll() is None:
            try:
                os.kill(self.proc.pid, signal.SIGCONT)
            except OSError:
                pass
            self.proc.kill()
        try:
            os.close(self.master)
        except OSError:
            pass


def pane_state(t, p):
    s = S.state()
    lc = s["lifecycle"]["panes"].get(t)
    surf = next((x for x in s["surfaces"] if x["pane"] == p), None)
    return lc, surf


def raw_osc(surf):
    return any("]4;" in l or ";rgb:" in l for l in (surf or {}).get("visible_nonblank", []))


def main():
    say(f"HerdrShell attach-swap check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    if "--no-build" not in sys.argv and "--app" not in sys.argv:
        r = subprocess.run(["swift", "build", "-c", "release", "--scratch-path", SCRATCH],
                           cwd=D, capture_output=True, text=True)
        say(f"swift build: {'ok' if r.returncode == 0 else 'FAILED'}")
        if r.returncode:
            say(r.stdout[-1500:] + r.stderr[-1500:])
            return finish()
    os.environ["HERDR_SHELL_APP"] = APP_BIN
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    spike = next(t["tab_id"] for t in snap["tabs"] if t["label"] == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == spike)
    p1 = min(lay["panes"], key=lambda p: p["rect"]["x"])["pane_id"]
    t1 = next(p["terminal_id"] for p in snap["panes"] if p["pane_id"] == p1)
    say(f"lab {S.NAME}; pane {p1} (terminal {t1}); app {APP_BIN} in the Space")
    if "--new-threads" in sys.argv:
        say(S.app("start").strip())
        wait(lambda: (lambda lc: lc if lc and lc["state"] == "running" else None)(pane_state(t1, p1)[0]), 20, 0.2)
        new_threads(spike)
        return finish()

    # ---- 1. the other machine sleeps while holding the pane -----------------------------
    say()
    say("== 1. a sleeping machine holds the pane; this machine opens it")
    book = Cli(t1, takeover=False)
    check("the other machine's client attaches to the pane", b"%" in book.drain(2.0) and book.proc.poll() is None)
    os.kill(book.proc.pid, signal.SIGSTOP)
    say(S.app("start").strip())
    notices, osc = set(), False
    t0 = time.time()

    def opened():
        nonlocal osc
        lc, surf = pane_state(t1, p1)
        if lc and lc.get("notice"):
            notices.add(lc["notice"])
        osc = osc or raw_osc(surf)
        if (lc and lc["state"] == "running" and not lc.get("notice") and surf and not surf["exited"]
                and any("%" in l for l in surf["visible_nonblank"])):
            return lc
        return None

    lc = wait(opened, 20, 0.2)
    final_lc, final_surf = pane_state(t1, p1)
    check("the pane opens attached with its screen", lc is not None,
          f"{time.time() - t0:.1f}s" if lc else f"state={final_lc and final_lc['state']} last_exit={final_lc and final_lc['last_exit']!r}")
    check("no banner on the pane at any point", not notices and not (final_lc or {}).get("notice"),
          "; ".join(sorted(notices)) or str((final_lc or {}).get("notice")))
    check("no raw OSC 4 color replies on the pane's screen", not osc and not raw_osc(final_surf),
          " | ".join(l for l in (final_surf or {}).get("visible_nonblank", []) if "]4;" in l or ";rgb:" in l)[:200])
    S.type_("echo SWAP-ONE")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "SWAP-ONE" for l in x.splitlines()))
    check("typing in the app reaches the pane", dt is not None, f"{dt:.2f}s" if dt is not None else txt[-160:])
    os.kill(book.proc.pid, signal.SIGCONT)
    gone = wait(lambda: book.drain(0.05) and book.proc.poll() is not None, 5)
    said = book.drain(0.3)
    check("the woken client finds the pane taken over and exits",
          gone is not None and b"taken over" in said,
          f"exit={book.proc.poll()} tail={said[-120:].decode(errors='replace')!r}")

    # ---- 2. the other machine takes it back, then the user returns here ------------------
    say()
    say("== 2. the other machine takes the pane; a click here takes it back")
    book2 = Cli(t1, takeover=True)
    book2.drain(1.0)
    held = wait(lambda: (lambda lc: lc if lc and lc["state"] == "held" else None)(pane_state(t1, p1)[0]), 8, 0.2)
    check("the app shows the pane held while the other machine has it", held is not None,
          (held or {}).get("notice") or "")
    # Since auto take-control (Alex, 2026-10-10: "take control should jsut automatically happen
    # in all clients"), keys typed at a held pane take it back once the holder is quiet and land.
    S.type_("echo TYPED-BACK")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: book2.drain(0.05) is not None and any(
        l.strip() == "TYPED-BACK" for l in x.splitlines()), timeout=8)
    lc_after_keys = pane_state(t1, p1)[0]
    check("keys typed at the held pane take it back and land",
          dt is not None and lc_after_keys and lc_after_keys["state"] == "running",
          f"state={lc_after_keys and lc_after_keys['state']} {txt[-120:]!r}")
    book3 = Cli(t1, takeover=True)
    book3.drain(1.0)
    wait(lambda: (lambda lc: lc if lc and lc["state"] == "held" else None)(pane_state(t1, p1)[0]), 8, 0.2)
    book2 = book3
    for action in ("down", "up"):
        S.cmd({"cmd": "mouse", "pane": p1, "action": action, "row": 12, "col": 10})
    back = wait(lambda: (lambda r: r[0] if r[0] and r[0]["state"] == "running" and r[1] and not r[1]["exited"]
                         and r[1]["visible_nonblank"] else None)(pane_state(t1, p1)), 8, 0.2)
    check("a click on the pane takes it back", back is not None, f"notice={(back or {}).get('notice')}")
    check("the other machine's client is shut out", wait(lambda: book2.proc.poll() is not None, 5) is not None)
    S.type_("echo SWAP-TWO")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "SWAP-TWO" for l in x.splitlines()))
    check("typing reaches the pane after taking it back", dt is not None, f"{dt:.2f}s" if dt is not None else txt[-160:])
    new_threads(spike)
    finish()


def panes_of(tab):
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    return {p["pane_id"]: p["terminal_id"] for p in snap["panes"] if p["tab_id"] == tab}


def new_threads(tab):
    """Alex, 2026-10-11: "new threads should be focused on the client that made them when
    they're first made." Every client showing the tab attaches a new pane as soon as it is
    listed; the one that made it keeps control."""
    # ---- 3. this app opens a new thread ---------------------------------------------------
    say()
    say("== 3. a new thread opened here is focused and controlled here at once")
    before = panes_of(tab)
    S.cmd({"cmd": "split", "direction": "right"})
    made = wait(lambda: [p for p in panes_of(tab) if p not in before], 5, 0.05)
    p3 = made[0] if made else None
    t3 = panes_of(tab).get(p3) if p3 else None
    # The other machine's client attaches the new pane as soon as it is listed, as an idle Shell does.
    other = Cli(t3, takeover=False) if t3 else None
    # Read its screen output as a live client does: a client nobody reads stalls the pane's output.
    pump = threading.Thread(target=lambda: other and other.drain(12.0), daemon=True)
    pump.start()
    shown = wait(lambda: (lambda lc, surf: lc if lc and lc["state"] == "running" and surf and surf["visible_nonblank"] else None)(
        *pane_state(t3, p3)), 8, 0.05) if p3 else None
    S.type_("echo NEW-HERE")
    S.key("return")
    txt, dt = S.wait_read(p3, lambda x: any(l.strip() == "NEW-HERE" for l in x.splitlines())) if p3 else ("", None)
    check("keys typed at once land in the new thread, no click", dt is not None,
          f"{dt:.2f}s" if dt is not None else f"pane={p3} shown={bool(shown)} {txt[-160:]!r}")
    lc3, _ = pane_state(t3, p3) if p3 else (None, None)
    check("this app holds the new thread", bool(lc3) and lc3["state"] == "running" and not lc3.get("notice"),
          f"state={lc3 and lc3['state']} notice={lc3 and lc3.get('notice')}")
    gone = wait(lambda: other.proc.poll() is not None, 5) if other else None
    pump.join()
    said = other.out.decode(errors="replace") if other else ""
    check("the other machine's client sees the thread but does not get control",
          gone is not None and ("already has an attached client" in said or "taken over" in said),
          f"exit={other and other.proc.poll()} tail={said[-120:]!r}")

    # ---- 4. another machine opens a new thread in the tab shown here ----------------------
    say()
    say("== 4. a new thread opened on another machine stays with that machine")
    before = panes_of(tab)
    S.lab("herdr", "pane", "split", list(before)[0], "--direction", "down")
    made = wait(lambda: [p for p in panes_of(tab) if p not in before], 5, 0.05)
    p4 = made[0] if made else None
    t4 = panes_of(tab).get(p4) if p4 else None
    # The machine that made it attaches it, taking it over if this app got there first.
    maker = Cli(t4, takeover=True) if t4 else None
    # The CLI client is drained while it waits: a client whose screen output nobody reads stops reading keys.
    seen = wait(lambda: maker.drain(0.1) is not None and (lambda lc: lc if lc and lc["state"] == "held" else None)(
        pane_state(t4, p4)[0]), 8, 0.1) if p4 else None
    maker and maker.drain(2.0)
    lc4, _ = pane_state(t4, p4) if p4 else (None, None)
    check("this app shows the new thread without control", seen is not None and lc4 and lc4["state"] == "held",
          f"state={lc4 and lc4['state']}")
    alive = maker is not None and maker.proc.poll() is None
    if alive:
        os.write(maker.master, b"echo MADE-THERE\r")
    txt, dt = S.wait_read(p4, lambda x: maker.drain(0.05) is not None and any(
        l.strip() == "MADE-THERE" for l in x.splitlines())) if alive else ("", None)
    check("the machine that made it keeps control: its keys land", alive and dt is not None,
          f"alive={alive} tail={(maker.drain(0.1)[-120:] if maker else b'')!r}")


def finish():
    for c in clients:
        c.close()
    try:
        S.app("stop")
    except Exception as e:  # noqa: BLE001
        say(f"app stop: {e}")
    S.lab("down")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL (' + ', '.join(failures) + ')'}")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    try:
        main()
    finally:
        for c in clients:
            c.close()
