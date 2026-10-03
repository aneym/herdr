#!/usr/bin/env python3
"""P30: server-side Park/Resume/Approve through local and shell-quoted ssh transports.

Run only in the shell lab's VM. The unblock stub records argv; lane.js writes real
fixture modes, so neither tool touches live state. Transport quoting and immediate
sidebar refresh are distinct contracts not covered by P26's local direct actions.
"""
import json
import os
import shutil
import shlex
import subprocess
import sys
import time
from pathlib import Path

os.environ["SHELL_LAB"] = "shellspike-p30"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
D = Path(__file__).resolve().parent.parent
LAB = Path.home() / ".cache/herdr-build/shellspike-p30"
FIX = LAB / "fixtures"
FIX.mkdir(parents=True, exist_ok=True)
os.environ["HERDR_SHELL_APP"] = str(LAB / "app/HerdrShell")
import scenario as S

OUT = Path(S.OUT) if "--out" in sys.argv else D / "checks/P30.txt"
lines, failures = [], []


def check(name, ok, detail=""):
    line = f"[{'PASS' if ok else 'FAIL'}] {name} {detail}"
    print(line, flush=True)
    lines.append(line)
    if not ok:
        failures.append(name)


def write(path, value):
    path.write_text(json.dumps(value))


def executable(path, source):
    path.write_text(source)
    path.chmod(0o755)


def wait(pred, timeout=15):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        state = S.state()
        if pred(state):
            return state
        time.sleep(0.1)
    return None


def read_fixture(path):
    if S.SPACE:
        S.pull(S.guest_path(str(path)), str(path))
    return path.read_text()


def receipt_ready(path, expected):
    try:
        return json.loads(read_fixture(path)) == expected
    except (RuntimeError, FileNotFoundError):
        return False


def main():
    helper = str(D / "bin/herdr-shell-remote")
    ping = subprocess.run(["python3", helper, "ping"], capture_output=True, text=True)
    check("helper ping", ping.returncode == 0 and json.loads(ping.stdout) == {"ok": True, "version": 1})
    # Override to a checkout's real lane.js if it is not installed in the VM.
    lane_bin = os.environ.get("HERDR_LANE_BIN", str(Path.home() / ".local/bin/herdr-lane"))
    if not Path(lane_bin).exists():
        check("real lane.js available", False, lane_bin)
        return
    if S.SPACE:
        S.space("exec", "/usr/bin/env python3 --version")
        S.space("node")
        from space import push_tree, push
        tree = Path(lane_bin).resolve().parent.parent
        guest_tree = "/Users/lume/.herdr-space/herdr-control"
        push_tree(str(tree), guest_tree)
        wrapper = FIX / "herdr-lane"
        executable(wrapper, '#!/bin/sh\nexec /Users/lume/.herdr-space/node/bin/node '
                   + guest_tree + '/bin/lane.js "$@"\n')
        lane_bin = str(wrapper)
    guest = lambda path: S.guest_path(str(path)) if S.SPACE else str(path)
    stub = FIX / "unblock"
    receipt = FIX / "approval.json"
    executable(stub, "#!/usr/bin/env python3\nimport json,sys\nfrom pathlib import Path\n"
               + f"Path({guest(receipt)!r}).write_text(json.dumps(sys.argv[1:]))\nprint('approved')\n")
    fake = FIX / "fake-ssh"
    executable(fake, "#!/usr/bin/env python3\nimport subprocess,sys\na=sys.argv[1:]\na.pop(0)\n"
               "while a and a[0]=='-o': del a[:2]\nsys.exit(subprocess.call(['sh','-c',' '.join(a)]))\n")
    workflows = FIX / "workflows"
    (workflows / "kinds").mkdir(parents=True, exist_ok=True)
    for transport in ("local", "ssh"):
        S.app("stop")
        S.lab("down")
        S.lab("up")
        env_text = S.lab("env")
        lab_home = next(line.split("=", 1)[1] for line in env_text.splitlines() if line.startswith("HOME="))
        (LAB / "bin").mkdir(exist_ok=True)
        herdr = LAB / "bin/herdr"
        if herdr.exists() or herdr.is_symlink():
            herdr.unlink()
        herdr.symlink_to(Path(lab_home).parent / "bin/herdr")
        snapshot = json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"]
        workspace = snapshot["workspaces"][0]["workspace_id"]
        tabs = [t for t in snapshot["tabs"] if t["workspace_id"] == workspace]
        tab = tabs[0]["tab_id"]
        slug = "p30-fixture"
        lane = {"tab": tab, "name": "remote scope", "label": "remote scope", "kind": "lane",
                "goal_area": "factory infra", "section": "scoping",
                "scope_url": f"https://example.invalid/?route=scoping/{slug}"}
        write(FIX / "lanes.json", {"version": 1, "lanes": [lane]})
        write(FIX / "areas.json", {"version": 1, "areas": [{"id": "factory", "name": "factory", "color": "#5AA9FF"}],
                                   "tabs": {}, "spaces": {workspace: "factory"}, "goal_area": {"factory infra": "factory"}})
        server_modes = FIX / "server-modes.json"
        client_modes = FIX / "client-modes.json"
        write(server_modes, {"version": 1, "tabs": {}})
        write(client_modes, {"version": 1, "tabs": {}})
        server = FIX / "server.json"
        write(server, {} if transport == "local" else {"ssh": [guest(fake), "studio"], "remote_bin": guest(helper)})
        # The lab launcher scrubs env. A lab-only executable wrapper passes the captured
        # production configuration without expanding the launcher's fixture allowlist.
        app_dir = LAB / "app"
        app_dir.mkdir(exist_ok=True)
        binary = app_dir / "HerdrShell-real"
        shutil.copy2(D / ".build/release/HerdrShell", binary)
        overrides = {"HERDR_LANES_PATH": str(FIX / "lanes.json"), "HERDR_AREAS_PATH": str(FIX / "areas.json"),
                     "CONTROL_MODES": str(client_modes), "CONTROL_WORKFLOWS": str(workflows),
                     "HERDR_KIND_BIN": "/usr/bin/true", "HERDR_LANE_BIN": lane_bin,
                     "UNBLOCK_BIN": str(stub), "HERDR_SHELL_SERVER_CONFIG": str(server), "HERDR_SHELL_REMOTE_BIN": helper}
        if transport == "local":
            # Local helper and app intentionally share modes, as on Studio.
            overrides["CONTROL_MODES"] = str(server_modes)
            client_modes = server_modes
        else:
            # The fake remote process owns a separate modes file, as Studio does on Book.
            source = fake.read_text().replace("import subprocess,sys", "import subprocess,sys,os")
            source = source.replace("sys.exit(subprocess.call", f"os.environ['CONTROL_MODES']={guest(server_modes)!r}\nsys.exit(subprocess.call")
            executable(fake, source)
        executable(app_dir / "HerdrShell", "#!/usr/bin/env python3\nimport os,sys\n"
                   + f"os.environ.update({overrides!r})\nos.execv({str(binary)!r}, [{str(binary)!r}]+sys.argv[1:])\n")
        if S.SPACE:
            os.environ.update(overrides)
            os.environ["HERDR_SHELL_APP"] = str(binary)
            push_tree(str(FIX), guest(FIX))
        S.app("start")
        # Spaces is the default since P33; these park/approve rows are asserted in the Areas view.
        wait(lambda s: s.get("shell", {}).get("mode") in ("spaces", "areas"))
        S.cmd({"cmd": "activate"})
        S.cmd({"cmd": "click", "target": "mode", "label": "areas"})
        ready = wait(lambda s: any(l.get("tab") == tab for l in s.get("sidebar_lines", [])))
        check(f"{transport}: fixture sidebar loaded", ready is not None)
        if ready is None:
            continue
        note = "a space, it's $(echo pwned)"
        S.cmd({"cmd": "sidebar_fold", "id": "parked", "open": True})
        S.cmd({"cmd": "park", "tab": tab, "note": note})
        state = wait(lambda s: s.get("last_remote", {}).get("verb") == "park" and s["last_remote"].get("ok") is True)
        started = time.monotonic()
        refreshed = state
        if state is not None and not any(l.get("tab") == tab and l.get("parked") for l in state.get("sidebar_lines", [])):
            refreshed = wait(lambda s: any(l.get("tab") == tab and l.get("parked") for l in s.get("sidebar_lines", [])), 2)
        refresh_elapsed = time.monotonic() - started
        modes = json.loads(read_fixture(server_modes))
        check(f"{transport}: exact park note", modes.get("tabs", {}).get(tab, {}).get("note") == note)
        check(f"{transport}: immediate modes sidebar", state is not None and state["last_remote"].get("ok") is True
              and state["last_remote"]["transport"] == transport
              and json.loads(read_fixture(client_modes)) == modes
              and refreshed is not None and any(l.get("tab") == tab and l.get("parked") for l in refreshed.get("sidebar_lines", []))
              and refresh_elapsed < 2)
        S.cmd({"cmd": "unpark", "tab": tab})
        state = wait(lambda s: s.get("last_remote", {}).get("verb") == "unpark")
        check(f"{transport}: resumed", state is not None and state["last_remote"].get("ok") is True
              and json.loads(read_fixture(server_modes)).get("tabs", {}).get(tab, {}).get("mode") != "parked")
        for note in ("-wip", "--", "--by=x"):
            S.cmd({"cmd": "park", "tab": tab, "note": note})
            state = wait(lambda s: s.get("last_remote", {}).get("verb") == "park"
                         and json.loads(read_fixture(server_modes)).get("tabs", {}).get(tab, {}).get("note") == note)
            check(f"{transport}: leading-dash note {note}", state is not None and state["last_remote"].get("ok") is True)
            S.cmd({"cmd": "unpark", "tab": tab})
            state = wait(lambda s: s.get("last_remote", {}).get("verb") == "unpark")
            check(f"{transport}: resume after {note}", state is not None and state["last_remote"].get("ok") is True)
        for quote in ('it\'s "good"', "-wip", "--", "--by=x"):
            if S.SPACE:
                S.space("exec", "rm -f " + shlex.quote(guest(receipt)))
            if receipt.exists():
                receipt.unlink()
            expected = ["scope", "approve", slug, "--by", "alex", "--quote", quote]
            S.cmd({"cmd": "approve", "tab": tab, "quote": quote})
            state = wait(lambda s: s.get("last_remote", {}).get("verb") == "approve"
                         and receipt_ready(receipt, expected))
            check(f"{transport}: exact approval argv {quote}", state is not None and state["last_remote"].get("ok") is True)
        before = read_fixture(receipt)
        bad = subprocess.run(["python3", helper, "approve", "../bad", "--quote=" + quote],
                             env={**os.environ, "UNBLOCK_BIN": str(stub)}, capture_output=True, text=True)
        check(f"{transport}: direct local helper rejects bad slug", bad.returncode == 1 and not json.loads(bad.stdout)["ok"]
              and read_fixture(receipt) == before)
        lane["scope_url"] = "https://example.invalid/?route=scoping/../bad"
        lane["name"] = "invalid scope fixture"
        write(FIX / "lanes.json", {"version": 1, "lanes": [lane]})
        if S.SPACE:
            push(str(FIX / "lanes.json"), guest(FIX / "lanes.json"))
        state = wait(lambda s: any(l.get("title") == "invalid scope fixture" for l in s.get("sidebar_lines", [])))
        check(f"{transport}: invalid scope fixture loaded", state is not None)
        S.cmd({"cmd": "approve", "tab": tab, "quote": quote})
        state = wait(lambda s: s.get("last_remote", {}).get("verb") == "approve"
                     and s.get("last_remote", {}).get("message") == "Invalid scope slug")
        check(f"{transport}: Swift hook rejects bad slug without tool execution", state is not None
              and state["last_remote"].get("ok") is False and read_fixture(receipt) == before)
        OUT.parent.mkdir(parents=True, exist_ok=True)
        shot = OUT.parent / f"P30-{transport}.png"
        S.cmd({"cmd": "shot", "out": str(shot)})
        end = time.monotonic() + 10
        while not shot.exists() and time.monotonic() < end:
            time.sleep(0.1)
        check(f"{transport}: screenshot", shot.exists() and shot.stat().st_size > 1000)
        S.app("stop")
        S.lab("down")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        import traceback
        traceback.print_exc()
        check("scenario exception", False, str(e))
    finally:
        S.app("stop")
        S.lab("down")
        OUT.parent.mkdir(parents=True, exist_ok=True)
        OUT.write_text("\n".join(lines) + "\n")
    sys.exit(bool(failures))
