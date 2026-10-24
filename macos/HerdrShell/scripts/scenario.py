#!/usr/bin/env python3
"""Scenario check for HerdrShell. Writes the report to --out (default checks/scenario.txt).
SHELL_LAB=<name> picks the lab session (default shellspike).

Fresh lab session -> launch app -> keys into the app (CGEvent
posted to the app's own pid through the window server, never system-wide) ->
verify with read-only `herdr pane read` on the lab session -> screenshot -> stop
the app and the lab session.
"""
import json
import os
import shlex
import subprocess
import sys
import time

D = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPACE = os.environ.get("HERDR_SHELL_SPACE") == "1"
GUEST_ROOT = "/Users/lume/.herdr-space/lab/"
NAME = os.environ.get("SHELL_LAB", "shellspike")
LAB = os.path.expanduser(f"~/.cache/herdr-build/{NAME}")
STATE = os.path.join(LAB, "state.json")
OUT = os.path.join(D, "checks", "scenario.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
SHOT = os.path.splitext(OUT)[0] + ".png"
lines = []
failures = []


def say(s=""):
    print(s)
    lines.append(s)


def sh(*a):
    return subprocess.run(a, capture_output=True, text=True).stdout


def lab(*a):
    return sh("python3", os.path.join(D, "scripts", "lab.py"), *a)


def herdr_json(*a):
    return json.loads(lab("herdr", *a))


_front_before = None


def frontmost_app():
    """Name of the front app. A check fails if this changes."""
    r = subprocess.run(
        ["osascript", "-e", 'tell application "System Events" to get name of first application process whose frontmost is true'],
        capture_output=True, text=True)
    name = (r.stdout or "").strip()
    if name:
        return name
    r = subprocess.run(["lsappinfo", "front"], capture_output=True, text=True)
    line = (r.stdout or "").splitlines()[0] if r.stdout else ""
    if ':"' in line:
        return line.split(':"', 1)[1].split('"', 1)[0]
    return line.strip() or "unknown"


def mark_front():
    global _front_before
    if _front_before is None:
        _front_before = frontmost_app()


def check_front(check):
    if SPACE:
        return
    now = frontmost_app()
    before = _front_before
    if before == "unknown" or now == "unknown":
        check("frontmost app unchanged", False, "unknown")
        return
    ok = before is not None and now == before
    check("frontmost app unchanged", ok, before if ok else "took focus")


def space(*a):
    r = subprocess.run([sys.executable, os.path.join(D, "scripts", "space.py"), *a],
                       capture_output=True, text=True)
    if r.returncode:
        raise RuntimeError(r.stderr or r.stdout)
    return r.stdout


def guest_path(host_path):
    path = os.path.abspath(os.path.expanduser(host_path))
    if path == LAB or path.startswith(LAB + os.sep):
        return GUEST_ROOT + NAME + path[len(LAB):]
    return GUEST_ROOT + NAME + "/paths" + path


def pull(guest, host):
    return space("pull", guest, host)


def app(*a):
    if SPACE:
        if a[0] == "start":
            env = dict(l.split("=", 1) for l in lab("env").splitlines())
            args = ["start", "--app", os.environ.get("HERDR_SHELL_APP") or
                    os.path.join(D, ".build", "release", "HerdrShell"),
                    "--socket", env["HERDR_SOCKET_PATH"]]
            forwarded = {"HERDR_LANES_PATH", "HERDR_AREAS_PATH", "HERDR_CONTEXT_DIR", "SHELL_LAB",
                         "FACTORY_OVERLAY", "CONTROL_MODES", "CONTROL_WORKFLOWS", "HERDR_KIND_BIN",
                         "HERDR_LANE_BIN", "UNBLOCK_BIN"}
            pushes = {}
            for k, value in os.environ.items():
                if k not in forwarded and not k.startswith("HERDR_SHELL_"):
                    continue
                if k in {"HERDR_SHELL_APP", "HERDR_SHELL_BIN", "HERDR_SHELL_SPACE"}:
                    continue
                if value.startswith("/") and not value.startswith("/usr/bin/"):
                    if not value.startswith("/Users/lume/.herdr-space/"):
                        target = guest_path(value)
                        if os.path.exists(value):
                            pushes[value] = target
                        value = target
                args += ["--env", k + "=" + value]
            for local, guest in pushes.items():
                args += ["--push", local + "=" + guest]
            keymap = os.path.join(D, "Resources", "keymap.json")
            args += ["--push", keymap + "=" + guest_path(keymap)]
            args += ["--env", "HERDR_SOCKET_PATH=/Users/lume/.herdr-space/herdr.sock",
                     "--env", "PATH=" + os.environ.get("SHELL_APP_PATH", "/Users/lume/.herdr-space/node/bin:/Users/lume/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin"),
                     "--", "--control", "/Users/lume/.herdr-space/control.fifo",
                     "--keymap", guest_path(keymap)]
            args += [x for x in a[1:] if x != "--agent-run"]
            return space(*args)
        if a[0] == "stop":
            # Checks stop first to clear their own leftovers; another seat's run is not theirs.
            return space("stop", "--if-mine")
        if a[0] == "cmd":
            return cmd(json.loads(a[1]))
    if a and a[0] == "start":
        mark_front()
    return sh("python3", os.path.join(D, "scripts", "app.py"), *a)


def cmd(obj):
    if SPACE:
        obj = dict(obj)
        host_out = obj.get("out")
        if host_out:
            obj["out"] = guest_path(host_out)
            space("exec", "mkdir -p " + shlex.quote(os.path.dirname(obj["out"]))
                  + " && rm -f " + shlex.quote(obj["out"]))
        space("fifo", json.dumps(obj))
        if host_out:
            path = shlex.quote(obj["out"])
            space("exec", f"for i in $(seq 1 100); do test -s {path} && exit 0; /bin/sleep 0.05; done; exit 1")
            pull(obj["out"], host_out)
        return
    app("cmd", json.dumps(obj))


def state():
    if os.path.exists(STATE):
        os.unlink(STATE)
    if SPACE:
        space("exec", "rm -f " + shlex.quote(guest_path(STATE)))
    cmd({"cmd": "state", "out": STATE})
    for _ in range(50):
        if os.path.exists(STATE) and os.path.getsize(STATE) > 0:
            time.sleep(0.05)
            return json.load(open(STATE))
        time.sleep(0.05)
    raise SystemExit("no state from app")


def pane_read(pane):
    return lab("herdr", "pane", "read", pane, "--source", "recent", "--lines", "40")


def wait_read(pane, pred, timeout=5.0):
    t0 = time.time()
    while time.time() - t0 < timeout:
        txt = pane_read(pane)
        if pred(txt):
            return txt, time.time() - t0
        time.sleep(0.02)
    return pane_read(pane), None


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def key(k, mods=(), via=None):
    body = {"cmd": "key", "key": k, "mods": list(mods)}
    if via:
        body["via"] = via
    cmd(body)


def type_(t, via=None):
    body = {"cmd": "type", "text": t}
    if via:
        body["via"] = via
    cmd(body)


def main():
    say(f"HerdrShell scenario  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    app("stop")
    lab("down")
    time.sleep(0.5)
    t = time.time()
    lab("up")
    say(f"lab session '{NAME}' up + seeded in {time.time() - t:.2f}s")
    say(f"herdr: {lab('herdr', '--version').strip()} (lab binary; needs --no-escape, spike branch spike/pane-attach until P1)")
    say(f"host load average: {os.getloadavg()[0]:.0f} on {os.cpu_count()} cores (timings below are under this load)")
    snap = herdr_json("api", "snapshot")["result"]["snapshot"]
    tabs = {x["tab_id"]: x["label"] for x in snap["tabs"]}
    spike_tab = next(k for k, v in tabs.items() if v == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == spike_tab)
    p1, p2 = [p["pane_id"] for p in sorted(lay["panes"], key=lambda p: p["rect"]["x"])]
    say(f"lab tabs: {json.dumps(tabs)}")
    say(f"scenario panes (plain zsh): pane1={p1} pane2={p2}")

    t = time.time()
    say(f"app start: {app('start').strip()}")
    ready = None
    for _ in range(200):
        s = state()
        surf = {x["pane"]: x for x in s["surfaces"]}
        if all(p in surf and any("%" in l for l in surf[p]["visible_nonblank"]) for p in (p1, p2)):
            ready = time.time() - t
            say(f"timing: surfaces created {s['app_age_s'] - max(x['age_s'] for x in s['surfaces']):.2f}s after app start; "
                "first Ghostty-rendered text " + ", ".join(f"{x['pane']} {x['first_text_after_s']:.2f}s" for x in s["surfaces"] if x["first_text_after_s"] is not None)
                + " after surface creation")
            break
        time.sleep(0.05)
    check("both panes attached and rendered by Ghostty surfaces", ready is not None,
          f"prompt visible in both surfaces {ready:.2f}s after launch" if ready else "")
    say("surfaces: " + ", ".join(f"{x['pane']} {x['terminal']} {x['cols']}x{x['rows']}" for x in s["surfaces"]))
    say(f"window: key={s['window_key']} app_active={s['app_active']} post_event_access={s['post_event_access']}")

    # Sidebar vs live herdr state.
    def labels(rows):
        out = []
        for r in rows:
            out.append(r["label"])
            out += labels(r["children"])
        return out
    side = s["sidebar"]
    side_labels = sorted(labels(side["orchestrator"]) + labels(side["lanes"]) + labels(side["workflows"]))
    check("sidebar lists every lab tab from live herdr state", side_labels == sorted(tabs.values()),
          f"sidebar={side_labels}")
    say("sidebar sections:")
    for sec in ("orchestrator", "lanes", "workflows"):
        for r in side[sec]:
            say(f"  {sec.upper():12} {r['label']:22} {r['status']:8} {r['host']:6} "
                f"folded wf: {[c['label'] + ' @' + c['host'] + ' ' + c['status'] for c in r['children']]}")
    check("folded workflow group under its owning lane",
          any(r["label"] == "recruiter" and [c["label"] for c in r["children"]] == ["wf recruiter-2320"]
              for r in side["lanes"]))
    # Live update: rename a tab through herdr and watch the sidebar follow.
    lab("herdr", "tab", "rename", next(k for k, v in tabs.items() if v == "recruiter"), "recruiter lane")
    t = time.time()
    seen = None
    while time.time() - t < 4:
        if "recruiter lane" in labels(state()["sidebar"]["lanes"]):
            seen = time.time() - t
            break
        time.sleep(0.05)
    check("sidebar follows a live herdr change (tab rename)", seen is not None,
          f"visible after {seen:.2f}s (1 s poll)" if seen is not None else "")

    # 1. Type into pane 1.
    s = state()
    check("pane 1 has keyboard focus on mount", s["focused_pane"] == p1, f"focused={s['focused_pane']}")
    t = time.time()
    type_("echo spike-ok-swift")
    key("return")
    txt, dt = wait_read(p1, lambda x: any(l.strip() == "spike-ok-swift" for l in x.splitlines()))
    check("typed 'echo spike-ok-swift' + Return reached pane 1 (herdr pane read)", dt is not None,
          f"output line seen {dt:.3f}s after the last key" if dt is not None else txt[-300:])
    say(f"herdr pane read {p1} --source recent:")
    for l in [l for l in txt.splitlines() if l.strip()][-4:]:
        say(f"  | {l}")

    # 2. App-level chord: cmd+] = next pane (menu item). Must switch focus and
    # must not reach either terminal.
    key("]", ["cmd"])
    time.sleep(0.2)
    s = state()
    check("cmd+] (app chord) moved focus pane1 -> pane2", s["focused_pane"] == p2, f"focused={s['focused_pane']}")
    txt1, txt2 = pane_read(p1), pane_read(p2)
    check("cmd+] did not leak into either pane", "]" not in txt1.splitlines()[-1] and "]" not in txt2)

    # 3. Terminal-owned keys in pane 2: ctrl+b (herdr's attach escape without
    # --no-escape), ctrl+c, option+backspace.
    type_("cat -v")
    key("return")
    wait_read(p2, lambda x: "cat -v" in x)
    time.sleep(0.2)
    key("b", ["ctrl"])
    key("return")
    txt, dt = wait_read(p2, lambda x: any(l.strip() == "^B" for l in x.splitlines()))
    check("ctrl+b reached pane 2 (cat -v printed ^B)", dt is not None, f"{dt:.3f}s" if dt is not None else txt[-200:])
    key("c", ["ctrl"])
    txt, dt = wait_read(p2, lambda x: "^C" in x and x.rstrip().endswith("%"))
    check("ctrl+c reached pane 2 (cat interrupted, prompt back)", dt is not None, f"{dt:.3f}s" if dt is not None else txt[-200:])
    type_("echo abc def")
    key("backspace", ["opt"])
    key("return")
    txt, dt = wait_read(p2, lambda x: any(l.strip() == "abc" for l in x.splitlines()))
    check("option+backspace reached pane 2 (zsh deleted the word 'def')", dt is not None,
          f"{dt:.3f}s" if dt is not None else txt[-200:])
    say(f"herdr pane read {p2} --source recent:")
    for l in [l for l in txt.splitlines() if l.strip()][-7:]:
        say(f"  | {l}")

    s = state()
    say(f"key delivery log (last 6): {s['delivered'][-6:]}")
    say(f"sidebar poll: herdr api snapshot {s['poll_ms']:.1f} ms")
    rss = sh("ps", "-o", "rss=,%cpu=", "-p", ",".join(sh("pgrep", "-f", os.path.join(D, ".build/release/HerdrShell")).split()))
    say(f"app rss_kb/%cpu: {rss.strip()}")

    # Screenshot.
    if os.path.exists(SHOT):
        os.unlink(SHOT)
    cmd({"cmd": "shot", "out": SHOT})
    time.sleep(1)
    say(f"screenshot: in-app cacheDisplay -> {os.path.basename(SHOT)} exists={os.path.exists(SHOT)}")
    check_front(check)

    app("stop")
    time.sleep(0.5)
    left = sh("pgrep", "-f", f"{NAME}/bin/herdr terminal attach").split()
    check("attach clients exit with the app", not left, f"left={left}")
    say(f"lab down: {lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
