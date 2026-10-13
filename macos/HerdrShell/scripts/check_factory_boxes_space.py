#!/usr/bin/env python3
"""R242, in the Cua Space: the Factory view draws one row per real machine, and every boxes.json
box sits under exactly one machine row that shows its load. In flight lists only runs whose
registry file the runner still rewrites.

Run: HERDR_SHELL_SPACE=1 python3 scripts/check_factory_boxes_space.py  (needs .build/release/HerdrShell)

verify-native 2026-10-13 found 1 of 7 boxes with a load and the overlay's "PC", "forge" and
"Mac Studio" hosts as extra rows: hosts and boxes were joined by lowercased name with no alias
map. In flight showed a 7-day-old run, because the Shell asked its own process table about
runner pids written on another machine (and on Studio a reboot reuses them).

Inputs are Studio's recorded factory files (windows/HerdrShell/app/src/fixtures/factory-boxes.json,
the PC vitest's fixture too): overlay hosts, boxes.json, the overlay machine registry, disk watch.
Two workflow records name the guest Dock's pid, a live process that is not a runner (what a
reboot leaves behind on Studio): one record rewritten now, one two hours ago. The app is `HerdrShell --demo factory --dump-factory`; asserts read the dump the
view draws from. Writes checks/FACTORY-BOXES.txt and light/dark shots; never launches on the host.
"""
import json
import os
import pathlib
import re
import socket
import subprocess
import sys
import tempfile
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_factory_boxes_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ.setdefault("HERDR_SPACE_OWNER", "check-factory-boxes")
ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
BIN = os.environ.get("HERDR_SHELL_APP") or str(ROOT / ".build/release/HerdrShell")
RECORDED = REPO / "windows/HerdrShell/app/src/fixtures/factory-boxes.json"
ROUTING = ROOT / "scripts/fixtures/p18/routing.json"
OUT = ROOT / "checks/FACTORY-BOXES.txt"
GUEST = "/Users/lume/.herdr-space/factory-boxes"
lines, failures = [], []


def check(name, ok, detail=""):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else "")
    print(line, flush=True)
    lines.append(line)
    if not ok:
        failures.append(name)


def space(*args, check_rc=True):
    r = subprocess.run([sys.executable, str(ROOT / "scripts/space.py"), *args], capture_output=True, text=True)
    if check_rc and r.returncode:
        raise RuntimeError(f"space {args[0]}: {r.stderr or r.stdout}")
    return r


def fixtures(root, pid):
    rec = json.loads(RECORDED.read_text())
    now = time.time()
    overlay = dict(rec["overlay"], generated_at=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now)))
    files = {"overlay.json": overlay, "boxes.json": rec["boxes"], "machines.json": rec["machines"],
             "disk.json": rec["disk"], "pool.json": {}, "routing.json": json.loads(ROUTING.read_text())}
    for name, body in files.items():
        (root / name).write_text(json.dumps(body))
    wf = root / "workflows"
    wf.mkdir()
    for run_id, lane, age in (("live-run", "native", 0), ("stale-run", "rooms-mac-sr1p", 7200)):
        path = wf / f"{run_id}.json"
        path.write_text(json.dumps({"run_id": run_id, "name": "fold-pipeline-v2", "lane": lane, "runner_pid": pid,
                                    "child_pid": pid, "started": now - age - 60, "headless": True}))
        os.utime(path, (now - age, now - age))
    return rec


def env_args():
    env = {"FACTORY_OVERLAY": "overlay.json", "FACTORY_BOXES": "boxes.json", "FACTORY_MACHINES": "machines.json",
           "FACTORY_DISK": "disk.json", "FACTORY_POOLSTATE": "pool.json", "FACTORY_ROUTING": "routing.json",
           "FACTORY_WORKFLOWS_DIR": "workflows", "FACTORY_REPO": "."}
    out = []
    for key, rel in env.items():
        out += ["--env", f"{key}={GUEST}/{rel}"]
    return out + ["--env", "FACTORY_POOLS_URL=http://127.0.0.1:9/api/pools"]


def run(mode, root, sock):
    dump = f"{GUEST}/{mode}.json"
    space("start", "--app", BIN, "--socket", sock, "--push", f"{root}={GUEST}", *env_args(),
          "--", "--demo", "factory", "--appearance", mode, "--dump-factory", dump)
    got = None
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        r = space("exec", f"cat {dump} 2>/dev/null; true", check_rc=False)
        try:
            got = json.loads(r.stdout)
            if got.get("machines") and space("exec", f"test -s {GUEST}/{mode}.png", check_rc=False).returncode == 0:
                break
        except ValueError:
            pass
        time.sleep(1)
    shot = ROOT / f"checks/FACTORY-BOXES-{mode}.png"
    space("pull", f"{GUEST}/{mode}.png", str(shot), check_rc=False)
    check(f"{mode} shot", shot.exists() and shot.stat().st_size > 0, str(shot))
    space("stop")
    return got or {}


def main():
    OUT.parent.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="factory-boxes-") as tmp:
        root = pathlib.Path(tmp) / "src"
        root.mkdir()
        space("stop", "--if-mine")
        pid = int(space("exec", "pgrep -x Dock | head -1").stdout.strip())
        rec = fixtures(root, pid)
        sock_path = os.path.join(tmp, "s.sock")
        sock = socket.socket(socket.AF_UNIX)
        sock.bind(sock_path)
        sock.listen()
        dumps = {mode: run(mode, root, sock_path) for mode in ("light", "dark")}
    got = dumps["light"]
    rows = got.get("machines", [])
    names = [m["name"] for m in rows]
    check("one row per real machine", names == ["Mac Studio", "PC", "ax42", "forge"], " ".join(names))
    for box in [b["name"] for b in rec["boxes"]["boxes"]]:
        owners = [m for m in rows if box in m.get("box_names", [])]
        drawn = owners and (owners[0]["name"] == box or owners[0].get("box") == box
                            or box in [b["name"] for b in owners[0].get("boxes", [])])
        load = owners[0].get("usage", "") if owners else ""
        check(f"box {box}: under one machine row with its load",
              len(owners) == 1 and bool(drawn) and re.search(r"\d\.\d\d/core", load) is not None,
              f"{owners[0]['name'] if owners else None}: {load}")
    forge = next((m for m in rows if m["name"] == "forge"), {})
    lanes = next((b for b in forge.get("boxes", []) if b["name"] == "forge-lanes"), {})
    check("forge slots group under forge, forge-lanes keeps its own held state",
          [b["name"] for b in forge.get("boxes", [])] == ["forge-1", "forge-2", "forge-3", "forge-4", "forge-lanes"]
          and lanes.get("state") == "held", json.dumps(forge.get("boxes"))[:200])
    studio = next((m for m in rows if m["name"] == "Mac Studio"), {})
    check("Mac Studio gets the disk watch's studio entry", studio.get("disk") == "ok", studio.get("disk", ""))
    flights = [f["id"] for f in got.get("flights", [])]
    check("in flight lists only the run whose file is still rewritten", flights == ["live-run"], str(flights))
    check("dark dump draws the same rows", [m["name"] for m in dumps["dark"].get("machines", [])] == names)
    OUT.write_text("\n".join(lines) + "\n")
    print(f"{len(lines) - len(failures)}/{len(lines)} passed; {OUT}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
