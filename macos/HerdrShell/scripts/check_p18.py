#!/usr/bin/env python3
"""P18 check: Factory view.

  python3 scripts/check_p18.py --out checks/P18.txt

Fixtures under scripts/fixtures/p18/ are copied to a temp dir. Every FACTORY_*
override points there. A local HTTP server serves the pools JSON. One workflow
file uses this process's pid (live) and one uses a dead pid. The app is
HerdrShell --demo factory --dump-factory <path>, which needs no herdr socket.

Asserts come from the JSON dump the app writes on each refresh.
"""
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from zoneinfo import ZoneInfo

D = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIX = os.path.join(D, "scripts", "fixtures", "p18")
BIN = os.environ.get("HERDR_SHELL_APP") or os.path.join(D, ".build", "release", "HerdrShell")
OUT = os.path.join(D, "checks", "P18.txt")
if "--out" in sys.argv:
    OUT = os.path.abspath(sys.argv[sys.argv.index("--out") + 1])
CHECKS = os.path.dirname(OUT)
ET = ZoneInfo("America/New_York")
DOWN_EPOCH = 2000000000

lines, failures = [], []
procs = []
front_before = ""


def say(s=""):
    print(s, flush=True)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def down_until():
    return datetime.fromtimestamp(DOWN_EPOCH, ET).strftime("%H:%M")


def et_dt(iso):
    return datetime.fromisoformat(iso.replace("Z", "+00:00")).astimezone(ET)


def clock(dt, minutes):
    hour = dt.hour % 12 or 12
    suffix = "AM" if dt.hour < 12 else "PM"
    if minutes:
        return f"{hour}:{dt.minute:02d} {suffix}"
    return f"{hour} {suffix}"


def reset_phrase(iso):
    dt = et_dt(iso)
    now = datetime.now(ET)
    words = clock(dt, dt.minute != 0)
    if dt.date() == now.date():
        return f"resets {words}"
    weekday = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][dt.weekday()]
    return f"resets {weekday} {words}"


def dead_pid():
    for pid in range(2_000_000, 2_000_050):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return pid
        except PermissionError:
            continue
    return 2_000_000


class PoolsHandler(BaseHTTPRequestHandler):
    path_file = ""

    def do_GET(self):
        if self.path.split("?", 1)[0] != "/api/pools":
            self.send_response(404)
            self.end_headers()
            return
        body = open(self.path_file, "rb").read()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        return


def serve(path, port):
    PoolsHandler.path_file = path
    httpd = ThreadingHTTPServer(("127.0.0.1", port), PoolsHandler)
    httpd.allow_reuse_address = True
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    return httpd


def stop_server(httpd):
    if httpd is None:
        return
    httpd.shutdown()
    httpd.server_close()


def prepare(root, live_pid):
    if os.path.exists(root):
        shutil.rmtree(root)
    shutil.copytree(FIX, root)
    wf = os.path.join(root, "workflows")
    os.makedirs(wf, exist_ok=True)
    now = time.time()
    live = {
        "run_id": "live-factory",
        "name": "ship-factory",
        "lane": "native",
        "tab": "w5H:t1",
        "pane": "w5H:p1",
        "runner_pid": live_pid,
        "started": now - 720,
        "log": os.path.join(root, "live.log"),
        "headless": True,
        "host": "host-2",
    }
    dead = {
        "run_id": "dead-factory",
        "name": "dead-workflow",
        "lane": "native",
        "tab": "w5H:t9",
        "pane": "w5H:p9",
        "runner_pid": dead_pid(),
        "started": now - 50,
        "log": os.path.join(root, "dead.log"),
        "headless": False,
    }
    json.dump(live, open(os.path.join(wf, "live.json"), "w"))
    json.dump(dead, open(os.path.join(wf, "dead.json"), "w"))
    overlay = json.load(open(os.path.join(root, "overlay.json")))
    overlay["generated_at"] = datetime.now(ET).astimezone(ZoneInfo("UTC")).strftime("%Y-%m-%dT%H:%M:%SZ")
    json.dump(overlay, open(os.path.join(root, "overlay.json"), "w"))
    disk = json.load(open(os.path.join(root, "disk.json")))
    for row in disk.values():
        row["last_run"]["at"] = now - 30
    json.dump(disk, open(os.path.join(root, "disk.json"), "w"))


def env_for(root, port):
    env = os.environ.copy()
    for key in list(env):
        if key.startswith("FACTORY_"):
            del env[key]
    env.update({
        "FACTORY_OVERLAY": os.path.join(root, "overlay.json"),
        "FACTORY_POOLS_URL": f"http://127.0.0.1:{port}/api/pools",
        "FACTORY_ROUTING": os.path.join(root, "routing.json"),
        "FACTORY_BOXES": os.path.join(root, "boxes.json"),
        "FACTORY_POOLSTATE": os.path.join(root, "pool.json"),
        "FACTORY_DISK": os.path.join(root, "disk.json"),
        "FACTORY_WORKFLOWS_DIR": os.path.join(root, "workflows"),
        "FACTORY_REPO": root,
        "FACTORY_POOLS_INTERVAL": "2",
    })
    return env


def launch(extra_env, args):
    proc = subprocess.Popen([BIN, *args], env=extra_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    procs.append(proc)
    return proc


def stop(proc):
    if proc.poll() is not None:
        return
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    for _ in range(50):
        if proc.poll() is not None:
            return
        time.sleep(0.1)
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def read_dump(path):
    try:
        return json.load(open(path))
    except (OSError, json.JSONDecodeError):
        return None


def wait_dump(path, pred, timeout):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        last = read_dump(path)
        if last is not None and pred(last):
            return last
        time.sleep(0.25)
    return last


def machine(dump, name):
    return next((m for m in dump.get("machines", []) if m.get("name") == name), None)


def pool(dump, pid):
    return next((p for p in dump.get("pools", []) if p.get("id") == pid), None)


def factory_visible(png):
    from PIL import Image
    img = Image.open(png).convert("RGB")
    hits = 0
    for y in range(70, img.height - 30, 5):
        lums = []
        for x in range(int(img.width * 0.22), img.width - 24, 8):
            r, g, b = img.getpixel((x, y))
            lums.append(0.2126 * r + 0.7152 * g + 0.0722 * b)
        if lums and max(lums) - min(lums) > 26:
            hits += 1
    return hits


def frontmost_app():
    r = subprocess.run(
        ["osascript", "-e", 'tell application "System Events" to get name of first application process whose frontmost is true'],
        capture_output=True, text=True)
    return (r.stdout or "").strip() or "unknown"


def copy_shot(dump_path, dest):
    src = os.path.splitext(dump_path)[0] + ".png"
    for _ in range(40):
        if os.path.exists(src) and os.path.getsize(src) > 0:
            shutil.copyfile(src, dest)
            return True
        time.sleep(0.25)
    return False


def sensitive(blob):
    text = json.dumps(blob)
    if "@" in text or "sk-" in text or "ghp_" in text or "gho_" in text:
        return True
    return re.search(r'(?i)\b(?:acct|account)[_-][A-Za-z0-9]{4,}\b', text) is not None


def main():
    global front_before
    front_before = frontmost_app()
    say(f"HerdrShell P18 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    say(f"binary: {BIN} exists={os.path.exists(BIN)}")
    if not os.path.exists(BIN):
        check("release binary exists", False, BIN)
        finish(1)
        return
    tmp = tempfile.mkdtemp(prefix="p18-")
    port = 18765
    httpd = None
    try:
        prepare(tmp, os.getpid())
        pools_path = os.path.join(tmp, "pools.json")
        httpd = serve(pools_path, port)
        dump = os.path.join(tmp, "light.json")
        proc = launch(env_for(tmp, port), ["--agent-run", "--demo", "factory", "--appearance", "light", "--dump-factory", dump])
        got = wait_dump(dump, lambda d: machine(d, "host-5") and pool(d, "alpha") and any(r.get("name") == "implement" for r in d.get("routing", [])), 25)
        alive = proc.poll() is None
        check("demo stays up and writes a dump", alive and got is not None, f"exit={proc.poll()}")
        if not got:
            stop(proc)
            finish(1)
            return

        names = [m["name"] for m in got["machines"]]
        check("every fixture box is shown",
              all(n in names for n in ("host-2", "host-3", "host-4", "host-5", "host-6", "host-8")),
              " ".join(names))
        until = down_until()
        states = {m["name"]: m["state"] for m in got["machines"]}
        check("drained, down, and held state words",
              states.get("host-5") == "drained (disk under floor)"
              and states.get("host-4") == f"down until {until} (banner timeout)"
              and states.get("host-8") == "held"
              and states.get("host-2") == "up",
              str({k: states.get(k) for k in ("host-5", "host-4", "host-8", "host-2")}))
        forge2 = machine(got, "host-6")
        check("disabled box is dimmed", forge2 is not None and forge2.get("dimmed") is True and forge2.get("state") == "up",
              str(forge2))
        studio = machine(got, "host-2")
        mac = machine(got, "host-7")
        check("attention and overlay summary",
              studio and studio.get("attention") == "warn" and "load 235/16" in studio.get("summary", "")
              and mac and mac.get("attention") == "act",
              f"studio={studio.get('attention') if studio else None} mac={mac.get('attention') if mac else None}")
        check("slots and disk",
              machine(got, "host-5").get("slots") == "1/1"
              and "8.2 GiB" in machine(got, "host-4").get("disk", "")
              and "low" in machine(got, "host-4").get("disk", ""),
              machine(got, "host-4").get("disk", ""))

        alpha, tight, empty = pool(got, "alpha"), pool(got, "tight"), pool(got, "empty")
        check("pool bars match fixture percents",
              alpha and alpha.get("five_hour") == 72 and alpha.get("weekly") == 55
              and tight and tight.get("five_hour") == 10 and tight.get("weekly") == 8
              and empty and empty.get("five_hour") == 0,
              f"alpha={alpha.get('five_hour') if alpha else None}")
        check("amber under 15% headroom and red at 0 usable",
              alpha and alpha.get("tone") == "ok" and tight and tight.get("tone") == "amber" and empty and empty.get("tone") == "red")
        five_reset = reset_phrase("2026-10-01T22:49:00Z")
        week_reset = reset_phrase("2026-10-02T16:00:00Z")
        refill = "+3 accounts " + clock(et_dt("2026-10-02T11:00:00Z"), True)
        check("reset times rendered in ET",
              alpha and five_reset in alpha.get("five_hour_label", "")
              and week_reset in alpha.get("weekly_label", "")
              and alpha.get("refill") == refill
              and empty and "monthly 40%" in empty.get("monthly", "")
              and week_reset in empty.get("monthly", ""),
              f"5h={alpha.get('five_hour_label') if alpha else None} expected {five_reset} refill={alpha.get('refill') if alpha else None} expected {refill}")

        routing = {r["name"]: r["chips"] for r in got.get("routing", [])}
        check("interim chains in order, not the stale classes chain",
              routing.get("implement") == ["grok-4.7-medium", "composer-2.5", "sol-medium", "swe-2-high", "sonnet stand-in"]
              and routing.get("mechanical") == ["composer-2.5", "grok-4.7-medium"]
              and routing.get("explore") == ["sol-low", "grok-latest-low"]
              and routing.get("review") == ["sol-high", "opus-high"]
              and got.get("ladder_mode") == "interim"
              and "STALE" not in json.dumps(got.get("routing")),
              str(routing.get("implement")))
        check("decider line", str(got.get("decider", "")).startswith("Decider:"), got.get("decider", ""))

        flights = got.get("flights", [])
        flight_names = [f.get("name") for f in flights]
        check("only the live workflow is in flight",
              flight_names == ["ship-factory"] and flights[0].get("headless") is True and flights[0].get("host") == "host-2"
              and flights[0].get("lane") == "native" and str(flights[0].get("age", "")).endswith("m"),
              str(flight_names))
        check("dump stores no emails or tokens", not sensitive(got))
        check("no-identities check catches bare account ids",
              sensitive({"note": "acct_9f3ab2c1"}) and sensitive({"note": "account_9f3ab2"}) and not sensitive(got))

        light_png = os.path.join(CHECKS, "P18-light.png")
        copied = copy_shot(dump, light_png)
        rows = factory_visible(light_png) if copied else 0
        machines = got.get("machines") or []
        check("light screenshot shows Factory rows and usage",
              copied and rows >= 8 and any(m.get("usage") for m in machines) and len(machines) >= 2,
              f"text_rows={rows} machines={len(machines)}")

        overlay_path = os.path.join(tmp, "overlay.json")
        overlay = json.load(open(overlay_path))
        for host in overlay["hosts"]:
            if host["name"] == "host-2":
                host["summary"] = "load 240/16 · changed marker"
        json.dump(overlay, open(overlay_path, "w"))
        os.utime(overlay_path, None)
        changed = wait_dump(dump, lambda d: machine(d, "host-2") and "changed marker" in machine(d, "host-2").get("summary", ""), 6)
        check("fixture file change shows within 6s",
              changed is not None and machine(changed, "host-2") and "changed marker" in machine(changed, "host-2").get("summary", ""))

        stop_server(httpd)
        httpd = None
        stale = wait_dump(dump, lambda d: d.get("pools_stale") is True and (d.get("pools_age_s") or 0) >= 2 and pool(d, "alpha") and pool(d, "alpha").get("five_hour") == 72, 20)
        check("pools outage keeps the last data and shows its age",
              stale is not None and stale.get("pools_stale") is True and pool(stale, "alpha") and pool(stale, "alpha").get("five_hour") == 72
              and (stale.get("pools_age_s") or 0) >= 2,
              f"stale={None if not stale else stale.get('pools_stale')} age={None if not stale else stale.get('pools_age_s')}")

        body = json.load(open(pools_path))
        body["pools"][0]["fiveHourRemainingPercent"] = 73
        json.dump(body, open(pools_path, "w"))
        httpd = serve(pools_path, port)
        recovered = wait_dump(dump, lambda d: pool(d, "alpha") and pool(d, "alpha").get("five_hour") == 73 and d.get("pools_stale") is False, 70)
        check("pools recover after the server restarts",
              recovered is not None and pool(recovered, "alpha") and pool(recovered, "alpha").get("five_hour") == 73,
              f"five_hour={None if not recovered or not pool(recovered, 'alpha') else pool(recovered, 'alpha').get('five_hour')}")
        stop(proc)

        dump_d = os.path.join(tmp, "dark.json")
        proc_d = launch(env_for(tmp, port), ["--agent-run", "--demo", "factory", "--appearance", "dark", "--dump-factory", dump_d])
        dark_ready = wait_dump(dump_d, lambda d: machine(d, "host-2"), 25)
        dark_png = os.path.join(CHECKS, "P18-dark.png")
        dark_copied = dark_ready is not None and proc_d.poll() is None and copy_shot(dump_d, dark_png)
        dark_rows = factory_visible(dark_png) if dark_copied else 0
        check("dark screenshot shows Factory rows and usage",
              dark_copied and dark_rows >= 8 and any((m.get("usage") for m in (dark_ready or {}).get("machines") or [])),
              f"text_rows={dark_rows}")
        stop(proc_d)
        stop_server(httpd)
        httpd = None

        live_env = os.environ.copy()
        for key in list(live_env):
            if key.startswith("FACTORY_"):
                del live_env[key]
        live_dump = os.path.join(tmp, "live.json")
        proc_l = launch(live_env, ["--agent-run", "--demo", "factory", "--dump-factory", live_dump])
        live = wait_dump(live_dump, lambda d: (d.get("machines") or d.get("pools")) and d.get("ladder_mode") and (d.get("landed_count") or 0) > 0, 25)
        live_png = os.path.join(CHECKS, "P18-live.png")
        crashed = proc_l.poll() is not None
        check("live sources do not crash", not crashed and live is not None, f"exit={proc_l.poll()}")
        check("live routing ladder loaded", bool(live and live.get("ladder_mode")), "" if not live else str(live.get("ladder_mode")))
        check("live landed today", bool(live and (live.get("landed_count") or 0) > 0), "" if not live else str(live.get("landed_count")))
        check("live screenshot", (not crashed) and copy_shot(live_dump, live_png), live_png)
        if live is not None:
            check("live dump stores no emails or tokens", not sensitive(live))
        say("idle CPU samples (ps %cpu, every 5s):")
        time.sleep(5)
        samples = []
        for i in range(12):
            if proc_l.poll() is not None:
                break
            out = subprocess.run(["ps", "-p", str(proc_l.pid), "-o", "%cpu="], capture_output=True, text=True).stdout.strip()
            try:
                samples.append(float(out))
            except ValueError:
                samples.append(None)
            say(f"  t={i * 5:02d}s  {out or '—'}")
            if i < 11:
                time.sleep(5)
        nums = [n for n in samples if n is not None]
        avg = sum(nums) / len(nums) if nums else None
        say(f"idle CPU average: {avg:.2f}%" if avg is not None else "idle CPU average: none")
        check("idle CPU stayed low", avg is not None and avg < 30 and len(nums) == 12, f"n={len(nums)} avg={avg}")
        stop(proc_l)
    finally:
        stop_server(httpd)
        for proc in procs:
            stop(proc)
        shutil.rmtree(tmp, ignore_errors=True)
    finish(1 if failures else 0)


def finish(code):
    say()
    front_now = frontmost_app()
    same = front_before not in ("", "unknown") and front_now == front_before
    say(f"[{'PASS' if same else 'FAIL'}] frontmost app unchanged -- {front_before if same else 'took focus'}")
    if not same:
        code = 1
    say(f"RESULT: {'PASS' if code == 0 and not failures else 'FAIL ' + ', '.join(failures)}")
    home = os.path.expanduser("~")
    body = [ln.replace(home, "~") for ln in lines]
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w") as f:
        f.write("\n".join(body) + "\n")
    sys.exit(code)


if __name__ == "__main__":
    main()
