#!/usr/bin/env python3
"""Machine-swap check: a pane another machine still holds opens here without a banner.

  python3 scripts/check_attach_swap.py [--no-build] [--app BIN] [--out checks/ATTACH-SWAP.txt]

--app runs another build (for example origin/main's, to see the check fail there).

Alex, 2026-10-10: "swapping between machines seems to get it stuck". The app runs in the
Cua Space (HERDR_SHELL_SPACE=1), reaching the lab server over the Space's socket bridge,
as a second machine reaches Studio over its tunnel. The first machine is a CLI
`herdr terminal attach` on the host.

  1. The host client attaches to a pane, then stops (SIGSTOP): a machine gone to sleep
     with its socket still open. The app starts: the pane attaches with no notice and no
     raw `]4;N;rgb:` color replies on screen, typing reaches it, and the stopped client
     is shut out when it wakes.
  2. The host takes the pane back with --takeover: the app shows it held, and keys typed
     in the app do not reach the pane. A click on the pane takes it back: typing works
     afterwards and the host client is shut out.
"""
import json
import os
import pty
import select
import signal
import subprocess
import sys
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
    S.type_("echo LEAKED")
    S.key("return")
    time.sleep(1.0)
    lc_after_keys = pane_state(t1, p1)[0]
    check("keys typed at the held pane neither reach it nor take it back",
          "LEAKED" not in S.pane_read(p1) and lc_after_keys and lc_after_keys["state"] == "held",
          f"state={lc_after_keys and lc_after_keys['state']}")
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
    finish()


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
