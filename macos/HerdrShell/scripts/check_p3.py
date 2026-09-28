#!/usr/bin/env python3
"""P3 check: live state by subscription (no timer poll).

  1. a `tab.rename` sent straight to the lab API socket is in the app's model
     within 200 ms (median and worst of several renames; measured from just
     before the request is sent to the app's own "applied" log line, same clock)
  2. an agent status report and a token report reach the sidebar (status and
     host change) without any other trigger
  3. while nothing changes for IDLE_S seconds: the app makes no snapshot
     request, and no `herdr api snapshot` process is a child of the app
  4. after the lab server is stopped and started again the sidebar recovers by
     itself (reconnect + resubscribe + snapshot)

Runs in the lab session only (SHELL_LAB, default shellspike-p3). Keys are never
sent. Writes the report to --out (default checks/P3.txt).
"""
import json
import os
import socket
import subprocess
import sys
import time

D = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NAME = os.environ.get("SHELL_LAB", "shellspike-p3")
os.environ["SHELL_LAB"] = NAME
LAB = os.path.expanduser(f"~/.cache/herdr-build/{NAME}")
STATE = os.path.join(LAB, "state.json")
APPLOG = os.path.join(LAB, "app.log")
SOCK = os.path.join(LAB, "h", ".config", "herdr", "sessions", NAME, "herdr.sock")
OUT = os.path.join(D, "checks", "P3.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
IDLE_S = 4.0
RENAMES = 5
BUDGET_MS = 200.0
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


def lab(*a):
    r = subprocess.run(["python3", os.path.join(D, "scripts", "lab.py"), *a], capture_output=True, text=True)
    if r.returncode != 0 or (r.stderr.strip() and a[:1] == ("herdr",)):
        say(f"  (lab {' '.join(a[:4])}: rc={r.returncode} {r.stderr.strip()[:200]})")
    return r.stdout


def app(*a):
    return sh("python3", os.path.join(D, "scripts", "app.py"), *a)


def state():
    if os.path.exists(STATE):
        os.unlink(STATE)
    app("cmd", json.dumps({"cmd": "state", "out": STATE}))
    for _ in range(100):
        if os.path.exists(STATE) and os.path.getsize(STATE) > 0:
            time.sleep(0.05)
            return json.load(open(STATE))
        time.sleep(0.05)
    raise SystemExit("no state from app")


def rows(s):
    out = []

    def walk(rs):
        for r in rs:
            out.append(r)
            walk(r["children"])
    for sec in ("orchestrator", "lanes", "workflows"):
        walk(s["sidebar"][sec])
    return out


def api(method, params, sock_path=None):
    c = socket.socket(socket.AF_UNIX)
    c.connect(sock_path or SOCK)
    c.sendall((json.dumps({"id": "check", "method": method, "params": params}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = c.recv(65536)
        if not chunk:
            break
        buf += chunk
    c.close()
    return json.loads(buf)


def applog(since):
    """(epoch, tabs) for every 'p3: applied' line after byte offset `since`."""
    out = []
    with open(APPLOG, "rb") as f:
        f.seek(since)
        for l in f.read().decode(errors="replace").splitlines():
            if "p3: applied epoch=" in l:
                rest = l.split("p3: applied epoch=", 1)[1]
                ep, _, tabs = rest.partition(" tabs=")
                out.append((float(ep), tabs.split("|")))
    return out


def snapshot_lines(since):
    with open(APPLOG, "rb") as f:
        f.seek(since)
        return [l for l in f.read().decode(errors="replace").splitlines() if "p3: snapshot #" in l]


def main():
    say(f"P3 check: live state by subscription  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    app("stop")
    lab("down")
    time.sleep(0.5)
    lab("up")
    say(f"lab session '{NAME}' up; host load {os.getloadavg()[0]:.0f} on {os.cpu_count()} cores")
    snap = json.loads(lab("herdr", "api", "snapshot"))["result"]["snapshot"]
    tabs = {t["label"]: t["tab_id"] for t in snap["tabs"]}
    ag = {a["pane_id"]: a for a in snap["agents"]}
    say(f"lab tabs: {list(tabs)}")

    say(f"app start: {app('start').strip()}")
    for _ in range(200):
        s = state()
        if len(rows(s)) >= len(tabs):
            break
        time.sleep(0.1)
    check("sidebar filled from the first snapshot", len(rows(s)) == len(tabs), f"{len(rows(s))} rows")
    time.sleep(1.0)

    # 1. Rename latency.
    lats = []
    tab_id = tabs["recruiter"]
    for i in range(RENAMES):
        label = f"recruiter r{i}"
        off = os.path.getsize(APPLOG)
        t_send = time.time()
        resp = api("tab.rename", {"tab_id": tab_id, "label": label})
        assert "error" not in resp, resp
        hit = None
        for _ in range(100):
            for ep, tl in applog(off):
                if label in tl and ep >= t_send:
                    hit = ep
                    break
            if hit:
                break
            time.sleep(0.01)
        lats.append((hit - t_send) * 1000 if hit else None)
        time.sleep(0.4)
    got = [x for x in lats if x is not None]
    say("rename -> model latencies (ms): " + ", ".join(f"{x:.0f}" if x is not None else "MISSED" for x in lats))
    check(f"tab rename in the model within {BUDGET_MS:.0f} ms (every one of {RENAMES})",
          len(got) == RENAMES and max(got) <= BUDGET_MS,
          f"median {sorted(got)[len(got)//2]:.0f} ms, worst {max(got):.0f} ms" if got else "none seen")
    time.sleep(0.5)
    s = state()
    check("sidebar row shows the last rename", any(r["label"] == f"recruiter r{RENAMES-1}" for r in rows(s)))

    # 2. Status and token changes arrive without another trigger.
    pane = next(p for p, a in ag.items() if a["tab_id"] == tab_id)
    lab("herdr", "pane", "report-agent", pane, "--source", "spike", "--agent", "claude", "--state", "blocked")
    seen = None
    t0 = time.time()
    while time.time() - t0 < 3:
        if any(r["tab"] == tab_id and r["status"] == "blocked" for r in rows(state())):
            seen = time.time() - t0
            break
    check("agent status change reaches the sidebar", seen is not None,
          f"visible {seen:.2f}s after the report command returned" if seen is not None else "")
    lab("herdr", "pane", "report-metadata", pane, "--source", "spike", "--token", "host=forge-1")
    seen = None
    t0 = time.time()
    while time.time() - t0 < 3:
        if any(r["tab"] == tab_id and r["host"] == "forge-1" for r in rows(state())):
            seen = time.time() - t0
            break
    check("host token change reaches the sidebar", seen is not None,
          f"visible {seen:.2f}s after the report command returned" if seen is not None else "")
    new = json.loads(lab("herdr", "tab", "create", "--workspace", snap["workspaces"][0]["workspace_id"],
                         "--label", "p3 new tab", "--cwd", "/tmp", "--no-focus"))["result"]["tab"]["tab_id"]
    time.sleep(0.5)
    check("new tab appears in the sidebar", any(r["tab"] == new for r in rows(state())))
    lab("herdr", "tab", "close", new)
    time.sleep(0.5)
    check("closed tab leaves the sidebar", not any(r["tab"] == new for r in rows(state())))

    # 3. Idle: no requests, no snapshot process.
    time.sleep(1.0)
    apppid = sh("pgrep", "-f", os.path.join(D, ".build/release/HerdrShell")).split()
    apppid = apppid[0] if apppid else None
    off = os.path.getsize(APPLOG)
    procs = 0
    t0 = time.time()
    while time.time() - t0 < IDLE_S:
        if apppid:
            out = sh("pgrep", "-P", apppid, "-f", "api snapshot")
            procs += len(out.split())
        time.sleep(0.02)
    n_req = len(snapshot_lines(off))
    check(f"no snapshot request while nothing changes ({IDLE_S:.0f} s idle)", n_req == 0, f"{n_req} requests logged by the app")
    check("no `herdr api snapshot` child process of the app while idle", apppid is not None and procs == 0,
          f"app pid {apppid}; {procs} sightings in {IDLE_S:.0f} s")

    # 4. Server restart: the sidebar recovers by itself.
    lab("down")
    time.sleep(1.5)
    say("lab server stopped")
    off = os.path.getsize(APPLOG)
    lab("up")
    seen = None
    t0 = time.time()
    while time.time() - t0 < 10:
        if any("shell spike" in tl for _, tl in applog(off)):
            seen = time.time() - t0
            break
        time.sleep(0.05)
    check("sidebar reconnects and resubscribes after the lab server restarts", seen is not None,
          f"new snapshot applied {seen:.2f}s after the server came back" if seen is not None else "")
    if seen is not None:
        off = os.path.getsize(APPLOG)
        new_tabs = json.loads(lab("herdr", "api", "snapshot"))["result"]["snapshot"]["tabs"]
        rid = next(t["tab_id"] for t in new_tabs if t["label"] == "recruiter")
        t_send = time.time()
        api("tab.rename", {"tab_id": rid, "label": "after restart"})
        hit = None
        for _ in range(200):
            for ep, tl in applog(off):
                if "after restart" in tl and ep >= t_send:
                    hit = (ep - t_send) * 1000
            if hit is not None:
                break
            time.sleep(0.01)
        check("rename after the restart still arrives by event", hit is not None,
              f"{hit:.0f} ms" if hit is not None else "")

    app("stop")
    time.sleep(0.5)
    lab("down")
    say()
    say("app.log p3 lines (last 12):")
    with open(APPLOG, "rb") as f:
        for l in [l for l in f.read().decode(errors="replace").splitlines() if "p3:" in l][-12:]:
            say("  " + l[:160])
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
