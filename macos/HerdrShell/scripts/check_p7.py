#!/usr/bin/env python3
"""P7 check: surface lifecycle.

  python3 scripts/check_p7.py --out checks/P7.txt

Runs in its own lab (SHELL_LAB=shellspike-p7) and its own build of the app
(~/.cache/herdr-build/p7-swift, built by this script), so it never stops another
piece's app or lab. The app is launched with the lab's scrubbed environment and only
ever talks to the lab session.

  1. kill -9 of one pane's `herdr terminal attach` brings the pane back with its screen
     within 1 s; the other pane's attach is untouched; typing works afterwards.
  2. A CLI `herdr terminal attach --takeover` (in a pty) takes the pane: the app shows the
     notice with who holds it, typing in the app does not reach the pane, the CLI holder
     still can type, and after "reclaim" (the Take back action) the app types again and
     the CLI client is shut down.
  3. Hidden-tab policy: `keep` leaves attaches running for tabs not on screen; `detach`
     releases them and reattaches with the screen when the tab is shown again. Attach
     count and app RSS are printed for both.
"""
import json
import os
import pty
import select
import signal
import subprocess
import sys
import time

os.environ["SHELL_LAB"] = "shellspike-p7"
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402  (helpers: lab, cmd, herdr_json, pane_read, wait_read)

D = S.D
LAB = S.LAB
LAB_BIN = os.path.join(LAB, "bin", "herdr")
SCRATCH = os.path.expanduser("~/.cache/herdr-build/p7-swift")
APP_BIN = os.path.join(SCRATCH, "release", "HerdrShell")
FIFO = os.path.join(LAB, "control.fifo")
_launched = False
STATE = os.path.join(LAB, "p7-state.json")
OUT = os.path.join(D, "checks", "P7.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
lines, failures = [], []


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def sh(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def herdr(*a):
    return S.lab("herdr", *a)


# ---- app control (own binary, own pid) -------------------------------------------------

def app_pids():
    return [int(p) for p in sh("pgrep", "-f", APP_BIN).split()]


def app_start():
    global _launched
    # app.py always passes --agent-run. This build lives outside .build/release.
    os.environ["HERDR_SHELL_APP"] = APP_BIN
    S.mark_front()
    say(S.app("start").strip())
    for _ in range(200):
        if os.path.exists(FIFO) and app_pids():
            _launched = True
            return app_pids()
        time.sleep(0.05)
    sys.exit("app did not start")


def app_stop():
    for p in app_pids():
        os.kill(p, signal.SIGTERM)


def hook(obj):
    """Post one line to the app's control FIFO. The reader reopens between lines, so a write can
    race its close (EPIPE); retry the whole line."""
    for i in range(20):
        try:
            with open(FIFO, "w") as f:
                f.write(json.dumps(obj) + "\n")
            return
        except BrokenPipeError:
            time.sleep(0.05)
    raise SystemExit("control FIFO refused a command 20 times")


def state(timeout=6.0, tries=3):
    """State read: poll for the file without fixed sleeps; re-ask if the app is slow (host load)."""
    for _ in range(tries):
        if os.path.exists(STATE):
            os.unlink(STATE)
        hook({"cmd": "state", "out": STATE})
        t0 = time.time()
        while time.time() - t0 < timeout:
            try:
                if os.path.getsize(STATE) > 0:
                    return json.load(open(STATE))
            except (OSError, ValueError):
                pass
            time.sleep(0.004)
    raise SystemExit("no state from app")


def key(k, mods=()):
    hook({"cmd": "key", "key": k, "mods": list(mods)})


def typ(t):
    hook({"cmd": "type", "text": t})


# ---- process helpers ---------------------------------------------------------------------

def attach_pids(tid=None):
    pat = f"{LAB_BIN} terminal attach" + (f" {tid}" if tid else "")
    out = []
    for p in sh("pgrep", "-f", pat).split():
        comm = sh("ps", "-o", "comm=", "-p", p).strip()
        if comm.endswith("herdr"):
            out.append(int(p))
    return sorted(out)


def rss_kb(pid):
    return int(sh("ps", "-o", "rss=", "-p", str(pid)).strip() or 0)


def surf(s, pane):
    return next((x for x in s["surfaces"] if x["pane"] == pane), None)


def wait(pred, timeout, step=0.01):
    t0 = time.time()
    while time.time() - t0 < timeout:
        v = pred()
        if v:
            return v, time.time() - t0
        time.sleep(step)
    return None, None


def main():
    say(f"HerdrShell P7 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    say(f"host load average: {os.getloadavg()[0]:.0f} on {os.cpu_count()} cores")
    r = subprocess.run(["swift", "build", "-c", "release", "--scratch-path", SCRATCH],
                       cwd=D, capture_output=True, text=True)
    say(f"swift build: {'ok' if r.returncode == 0 else 'FAILED'}")
    if r.returncode != 0:
        say(r.stdout[-1500:] + r.stderr[-1500:])
        return finish()

    app_stop()
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    tabs = {t["label"]: t["tab_id"] for t in snap["tabs"]}
    spike = tabs["shell spike"]
    other = tabs["recruiter"]
    lay = next(l for l in snap["layouts"] if l["tab_id"] == spike)
    p1, p2 = [p["pane_id"] for p in sorted(lay["panes"], key=lambda p: p["rect"]["x"])]
    term = {p["pane_id"]: p["terminal_id"] for p in snap["panes"]}
    t1, t2 = term[p1], term[p2]
    say(f"lab session shellspike-p7; panes {p1} (terminal {t1}), {p2} (terminal {t2})")

    pids = app_start()
    say(f"app (own build) pid {pids}")
    ready, _ = wait(lambda: (lambda s: all(surf(s, p) and any("%" in l for l in surf(s, p)["visible_nonblank"])
                                          for p in (p1, p2)) and s)(state()), 30, 0.05)
    check("both panes attached and showing a prompt", ready is not None)
    if ready is None:
        return finish()
    app_pid = app_pids()[0]

    # ---- 1. kill one attach process ---------------------------------------------------
    say()
    say("== 1. kill of one attach process")
    typ("echo P7-marker-one")
    key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "P7-marker-one" for l in x.splitlines()))
    check("typed marker reached pane 1", dt is not None)
    s0 = state()
    check("pane 1 has keyboard focus before the kill", s0["focused_pane"] == p1, f"focused={s0['focused_pane']}")
    pid1, pid2 = attach_pids(t1), attach_pids(t2)
    check("one attach process per pane", len(pid1) == 1 and len(pid2) == 1, f"p1={pid1} p2={pid2}")
    t_kill = time.time()
    for p in pid1:
        os.kill(p, signal.SIGKILL)

    def back():
        s = state()
        x = surf(s, p1)
        lc = s["lifecycle"]["panes"].get(t1)
        if (x and lc and lc["respawns"] >= 1 and not x["exited"]
                and any("P7-marker-one" in l for l in x["visible_nonblank"])):
            return s
        return None

    s1, _ = wait(back, 5, 0.0)
    took = time.time() - t_kill if s1 else None
    check("pane 1 is back with its screen (marker visible in the new surface) within 1 s of the kill",
          s1 is not None and took < 1.0, f"{took:.3f}s" if took is not None else "never came back")
    if s1:
        lc = s1["lifecycle"]["panes"][t1]
        say(f"   app-side: respawns={lc['respawns']} exit seen -> first screen {lc['last_recover_s'] or 0:.3f}s state={lc['state']}")
        check("app counted one respawn, state running", lc["respawns"] == 1 and lc["state"] == "running")
        check("keyboard focus stayed on pane 1", s1["focused_pane"] == p1, f"focused={s1['focused_pane']}")
    new1, new2 = attach_pids(t1), attach_pids(t2)
    check("a new attach process serves pane 1; pane 2's attach is the same process",
          len(new1) == 1 and new1 != pid1 and new2 == pid2, f"p1 {pid1}->{new1}, p2 {pid2}->{new2}")
    typ("echo P7-after-respawn")
    key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "P7-after-respawn" for l in x.splitlines()))
    check("typing reaches pane 1 after the respawn", dt is not None, f"{dt:.3f}s" if dt is not None else txt[-200:])

    # ---- 1b. poisoned viewport: the pane itself says "taken over" ------------------------
    say()
    say("== 1b. kill while the pane displays the words a takeover uses")
    typ("echo 'herdr: terminal attach taken over'; echo 'terminal already has an attached client'; echo P7-poison-done")
    key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "P7-poison-done" for l in x.splitlines()))
    check("pane 1 displays 'taken over' as ordinary output", dt is not None and "herdr: terminal attach taken over" in txt)
    r0 = state()["lifecycle"]["panes"][t1]["respawns"]
    pid1b = attach_pids(t1)
    for p in pid1b:
        os.kill(p, signal.SIGKILL)

    def back_poison():
        s = state()
        x = surf(s, p1)
        lc = s["lifecycle"]["panes"].get(t1)
        if (x and lc and lc["respawns"] > r0 and lc["state"] == "running" and not x["exited"]
                and any("P7-poison-done" in l for l in x["visible_nonblank"])):
            return s
        return None

    s1b, _ = wait(back_poison, 5, 0.02)
    lc1b = state()["lifecycle"]["panes"][t1]
    check("killed attach with 'taken over' on screen is respawned, not marked held", s1b is not None,
          f"state={lc1b['state']} respawns={lc1b['respawns']} (was {r0})")
    n1b = attach_pids(t1)
    check("a new attach process serves pane 1", len(n1b) == 1 and n1b != pid1b, f"{pid1b}->{n1b}")
    typ("echo P7-after-poison")
    key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "P7-after-poison" for l in x.splitlines()))
    check("typing reaches pane 1 after the poisoned-viewport respawn", dt is not None,
          f"{dt:.3f}s" if dt is not None else txt[-200:])

    # ---- 2. takeover from a CLI client --------------------------------------------------
    say()
    say("== 2. --takeover from a CLI client")
    env = {l.split("=", 1)[0]: l.split("=", 1)[1] for l in S.lab("env").splitlines() if "=" in l}
    master, slave = pty.openpty()
    subprocess.run(["stty", "rows", "30", "cols", "100"], stdin=slave)
    cli = subprocess.Popen([LAB_BIN, "--session", "shellspike-p7", "terminal", "attach", t1, "--takeover", "--no-escape"],
                           stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
    os.close(slave)

    def drain(sec):
        end = time.time() + sec
        buf = b""
        while time.time() < end:
            r, _, _ = select.select([master], [], [], 0.05)
            if r:
                try:
                    buf += os.read(master, 65536)
                except OSError:
                    break
        return buf

    def held():
        s = state()
        lc = s["lifecycle"]["panes"].get(t1)
        return s if lc and lc["state"] == "held" else None

    sh_, _ = wait(held, 5, 0.02)
    drain(0.3)
    check("app shows pane 1 as held after the CLI takeover", sh_ is not None)
    if sh_:
        lc = sh_["lifecycle"]["panes"][t1]
        w, _ = wait(lambda: (lambda l: l if l["holder"] and str(cli.pid) in str(l["holder"]) else None)(
            state()["lifecycle"]["panes"][t1]), 5, 0.1)
        say(f"   notice: {w['notice'] if w else lc['notice']}")
        say(f"   holder: {w['holder'] if w else lc['holder']}")
        check("notice names the holding client (pid of the CLI attach)", w is not None, f"cli pid {cli.pid}")
        check("keyboard focus is not on the ended surface", not surf(sh_, p1)["first_responder"])
    # Typing in the app must not reach the pane.
    typ("echo leaked-into-pane")
    key("return")
    time.sleep(1.0)
    txt = S.pane_read(p1)
    check("typing in the app does not reach the held pane", "leaked-into-pane" not in txt, txt[-160:].replace("\n", " | "))
    # The CLI holder is the writer now.
    os.write(master, b"echo via-cli-holder\r")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "via-cli-holder" for l in x.splitlines()))
    check("the CLI holder can type into the pane (the app did not steal it back)", dt is not None,
          f"{dt:.3f}s" if dt is not None else txt[-200:])
    check("CLI holder still attached while the app is held", cli.poll() is None)
    # Take back.
    t_rc = time.time()
    hook({"cmd": "reclaim"})

    def reclaimed():
        s = state()
        lc = s["lifecycle"]["panes"].get(t1)
        x = surf(s, p1)
        return s if lc and lc["state"] == "running" and x and not x["exited"] and x["visible_nonblank"] else None

    rs, _ = wait(reclaimed, 5, 0.02)
    check("Take back re-attaches the pane with its screen", rs is not None, f"{time.time() - t_rc:.2f}s")
    gone, _ = wait(lambda: cli.poll() is not None, 5, 0.05)
    check("the CLI client was shut down by the takeback", gone is not None)
    check("notice is gone after taking back",
          rs is not None and rs["lifecycle"]["panes"][t1]["notice"] is None)
    if rs:
        check("keyboard focus is on pane 1 again", rs["focused_pane"] == p1, f"focused={rs['focused_pane']}")
    typ("echo P7-reclaimed")
    key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "P7-reclaimed" for l in x.splitlines()))
    check("typing reaches pane 1 after taking back", dt is not None, f"{dt:.3f}s" if dt is not None else txt[-200:])
    try:
        os.close(master)
    except OSError:
        pass
    if cli.poll() is None:
        cli.kill()

    # ---- 3. hidden-tab policy -----------------------------------------------------------
    say()
    say("== 3. hidden-tab policy (measured)")
    # Visit every tab so every pane has a surface.
    all_tabs = [t["tab_id"] for t in snap["tabs"]]
    for tb in all_tabs:
        hook({"cmd": "select", "tab": tb})
        time.sleep(0.5)
    hook({"cmd": "select", "tab": other})
    time.sleep(1.0)
    s = state()
    keep_attach = len(attach_pids())
    keep_rss = rss_kb(app_pid)
    say(f"policy=keep : hidden_policy={s['lifecycle']['hidden_policy']} attach processes={keep_attach} "
        f"(panes in snapshot {len(snap['panes'])}) app rss={keep_rss / 1024:.0f} MB")
    check("keep: panes of tabs not on screen stay attached", keep_attach == len(snap["panes"]) and len(pid1) == 1,
          f"{keep_attach} of {len(snap['panes'])}")
    check("keep: the hidden pane 1 attach process is still alive", len(attach_pids(t1)) == 1)

    hook({"cmd": "hidden_policy", "value": "detach"})
    dropped, dt = wait(lambda: len(attach_pids()) == 1, 8, 0.1)
    time.sleep(0.5)
    detach_attach = len(attach_pids())
    detach_rss = rss_kb(app_pid)
    say(f"policy=detach: attach processes={detach_attach} app rss={detach_rss / 1024:.0f} MB "
        f"(released in {dt:.1f}s)" if dt is not None else f"policy=detach: attach processes={detach_attach}")
    check("detach: only the visible tab's pane keeps an attach", detach_attach == 1, f"{detach_attach} attach processes")
    check("detach: hidden pane 1 has no attach process", len(attach_pids(t1)) == 0)
    t_show = time.time()
    hook({"cmd": "select", "tab": spike})

    def shown():
        s = state()
        a, b = surf(s, p1), surf(s, p2)
        if a and b and any("P7-reclaimed" in l for l in a["visible_nonblank"]) and any("%" in l for l in b["visible_nonblank"]):
            return s
        return None

    sw, _ = wait(shown, 8, 0.0)
    check("detach: showing the tab again reattaches both panes with their screens", sw is not None,
          f"{time.time() - t_show:.2f}s" if sw else "")
    typ("echo P7-after-detach")
    key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "P7-after-detach" for l in x.splitlines()))
    check("detach: typing reaches the pane after the reattach", dt is not None)
    # Flip away and straight back in detach mode: the reattach must not find the pane held.
    hook({"cmd": "select", "tab": other})
    time.sleep(0.05)
    hook({"cmd": "select", "tab": spike})
    t_flip = time.time()

    def flipped():
        s = state()
        a, b = surf(s, p1), surf(s, p2)
        lcs = s["lifecycle"]["panes"]
        ok = (a and b and not a["exited"] and not b["exited"] and a["visible_nonblank"] and b["visible_nonblank"]
              and all(lcs[t]["state"] == "running" for t in (t1, t2)))
        return s if ok else None

    fl, _ = wait(flipped, 8, 0.0)
    time.sleep(1.0)
    fl2 = state()
    check("detach: flipping away and straight back reattaches both panes (never shown as held)",
          fl is not None and all(fl2["lifecycle"]["panes"][t]["state"] == "running" for t in (t1, t2))
          and len(attach_pids(t1)) == 1 and len(attach_pids(t2)) == 1,
          f"{time.time() - t_flip:.2f}s; respawns p1={fl2['lifecycle']['panes'][t1]['respawns']}")
    say(f"measured: keep = {keep_attach} attach clients, {keep_rss / 1024:.0f} MB; "
        f"detach = {detach_attach} attach clients, {detach_rss / 1024:.0f} MB (this lab: {len(snap['panes'])} panes, "
        "one tab on screen). Default stays keep.")
    hook({"cmd": "hidden_policy", "value": "keep"})
    finish()


def finish():
    if _launched:
        S.check_front(check)
    app_stop()
    time.sleep(0.6)
    left = attach_pids()
    check("no attach clients left after the app quits", not left, f"left={left}")
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
