#!/usr/bin/env python3
import json
import os
import pathlib
import shlex
import subprocess
import sys
import time
import uuid

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_thoughts_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
if os.environ.get("HERDR_SPACE_OWNER") != "thoughts-slot":
    raise SystemExit("check_thoughts_space requires HERDR_SPACE_OWNER=thoughts-slot")
if os.environ.get("HERDR_SPACE", "herdr-qa") != "herdr-qa":
    raise SystemExit("check_thoughts_space requires the herdr-qa Space")
os.environ["SHELL_LAB"] = "shellspike-t"
os.environ["SHELL_LAB_DIR"] = os.path.expanduser("~/.cache/herdr-build/t")
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S

S.LAB = os.environ["SHELL_LAB_DIR"]
S.STATE = os.path.join(S.LAB, "state.json")
S.OUT = str(ROOT / "checks/THOUGHTS-SPACE.txt")
ARTIFACTS = pathlib.Path("/Users/aneyman/.agent-rails/lanes/thoughts-slot")
GUEST = "/Users/lume/.herdr-space/thoughts-check-" + uuid.uuid4().hex
TEXTS = ["An unorganized thought from the Mac slot", "First line of another thought\nSecond line stays separate\n"]
lines, failures = [], []


def say(text):
    print(text, flush=True)
    lines.append(text)


def check(name, condition, detail=""):
    say(f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not condition:
        failures.append(name)
        raise RuntimeError(name)


def lab(*args):
    result = subprocess.run([sys.executable, str(ROOT / "scripts/lab.py"), *args],
                            capture_output=True, text=True, timeout=90)
    if result.returncode:
        raise RuntimeError(result.stderr or result.stdout)
    return result.stdout


def wait(predicate, timeout=35):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.4)
    raise RuntimeError("state did not settle: " + json.dumps(state, default=str)[-2000:])


def guest_python(code):
    return S.space("exec", "python3 -c " + shlex.quote(code))


def records():
    return json.loads(guest_python(
        "import pathlib,json; p=pathlib.Path(" + repr(GUEST + "/inbox") + "); "
        "print(json.dumps([{'name':f.name,'text':f.read_text()} for f in sorted(p.glob('*.md')) "
        "if not f.name.startswith('.')]))"))


def shot(name, sidebar=False):
    from PIL import Image
    path = ARTIFACTS / name
    time.sleep(0.5)
    state = S.state()
    S.cmd({"cmd": "shot", "out": str(path), "scale": 2})
    if sidebar:
        def frame(text):
            return [float(v) for v in text.replace("{", "").replace("}", "").split(",")]
        with Image.open(path) as image:
            scale = image.width / frame(state["window_frame"])[2]
            x, y, width, height = frame(state["theme"]["sidebar_frame"])
            image.crop((round(x * scale), round(y * scale),
                        round((x + width) * scale), round((y + height) * scale))).save(path)
    check("screenshot " + name, path.is_file() and path.stat().st_size > 0)


def main():
    binary = ROOT / ".build/release/HerdrShell"
    inputs = [ROOT / "Package.swift"]
    inputs += [p for directory in ("Sources", "Resources") for p in (ROOT / directory).rglob("*") if p.is_file()]
    if not binary.exists() or any(p.stat().st_mtime > binary.stat().st_mtime for p in inputs):
        subprocess.run(["just", "shell-build"], cwd=ROOT.parents[1], check=True, timeout=260)
        say("PASS just shell-build")
    else:
        say("Release binary is current against Package.swift, Sources and Resources")
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    S.app("stop")
    lab("down")
    lab("up")
    say("Started isolated lab shellspike-t")
    env = dict(line.split("=", 1) for line in lab("env").splitlines())
    say(S.space("start", "--app", str(binary), "--socket", env["HERDR_SOCKET_PATH"],
                "--wait", "120",
                "--env", "SHELL_LAB=" + S.NAME,
                "--env", "HERDR_THOUGHTS_DIR=" + GUEST,
                "--env", "HERDR_SHELL_SERVER_CONFIG=" + GUEST + "/no-server.json",
                "--env", "HERDR_SHELL_REMOTE_BIN=" + GUEST + "/herdr-shell-remote",
                "--env", "HERDR_SOCKET_PATH=/Users/lume/.herdr-space/herdr.sock",
                "--env", "PATH=/Users/lume/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                "--push", str(ROOT / "bin/herdr-shell-remote") + "=" + GUEST + "/herdr-shell-remote",
                "--push", str(ROOT / "Resources/keymap.json") + "=" + GUEST + "/keymap.json",
                "--", "--control", GUEST + "/control.fifo",
                "--keymap", GUEST + "/keymap.json").strip())
    guest_python("import os,time\n"
                 "deadline=time.monotonic()+60\n"
                 "while time.monotonic()<deadline:\n"
                 " try:\n"
                 "  fd=os.open(" + repr(GUEST + "/control.fifo") + ",os.O_WRONLY|os.O_NONBLOCK)\n"
                 "  os.close(fd)\n"
                 "  break\n"
                 " except OSError:\n"
                 "  time.sleep(0.2)\n"
                 "else:\n"
                 " raise RuntimeError('fresh app control FIFO did not become ready')")
    check("fresh app control FIFO ready", True)
    guest_python("import pathlib; p=pathlib.Path(" + repr(GUEST) + "); "
                 "(p/'inbox').mkdir(exist_ok=True); (p/'done').mkdir(exist_ok=True)")
    check("isolated guest inbox starts empty", records() == [], GUEST)
    wait(lambda s: s.get("thoughts_count") == 0)
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "appearance", "mode": "light"})
    wait(lambda s: s.get("theme", {}).get("effective") == "light")
    for index, text in enumerate(TEXTS):
        S.cmd({"cmd": "click", "target": "thoughts"})
        wait(lambda s: s.get("thoughts_open") is True)
        check("Thoughts row opens capture " + str(index + 1), True)
        S.cmd({"cmd": "thoughts_type", "text": text})
        if index == 1:
            shot("step1-capture-open.png")
        S.cmd({"cmd": "thoughts_save"})
        state = wait(lambda s: s.get("thoughts_count") == index + 1 and s.get("thoughts_open") is False)
        check("saved thought " + str(index + 1), state["thoughts_count"] == index + 1)
    inbox = records()
    check("inbox contains exactly two files", len(inbox) == 2)
    bodies = []
    for record in inbox:
        say("FILE " + record["name"] + "\n" + record["text"])
        bodies.append(record["text"].split("\n---\n", 1)[1])
    check("stored text preserves both thoughts including multiline bytes", sorted(bodies) == sorted(TEXTS))
    check("state thoughts_count == 2", S.state().get("thoughts_count") == 2)
    shot("step1-thoughts-2.png", sidebar=True)
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        wait(lambda s: s.get("theme", {}).get("effective") == mode)
        shot("mac-thoughts-" + mode + ".png", sidebar=True)
        S.cmd({"cmd": "thoughts_open"})
        wait(lambda s: s.get("thoughts_open") is True)
        S.cmd({"cmd": "thoughts_type", "text": "Unorganized thoughts\nA little space to capture them."})
        shot("mac-thoughts-capture-" + mode + ".png")
        S.cmd({"cmd": "key", "key": "escape"})
        wait(lambda s: s.get("thoughts_open") is False)
    guest_python("import pathlib; p=pathlib.Path(" + repr(GUEST) + "); "
                 "f=p/'inbox'/" + repr(inbox[0]["name"]) + "; f.rename(p/'done'/f.name)")
    state = wait(lambda s: s.get("thoughts_count") == 1, timeout=45)
    check("poll refreshes state thoughts_count == 1 after archival", state["thoughts_count"] == 1)
    check("inbox now contains exactly one file", len(records()) == 1)
    S.cmd({"cmd": "appearance", "mode": "light"})
    wait(lambda s: s.get("theme", {}).get("effective") == "light")
    shot("step2-thoughts-1.png", sidebar=True)


if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        failures.append(str(exc))
        say("[FAIL] scenario completed -- " + str(exc))
        try:
            say(S.space("exec", "tail -40 /Users/lume/.herdr-space/app.log"))
        except (Exception, SystemExit) as diagnostic:
            say("Guest log unavailable: " + str(diagnostic))
    finally:
        for name, cleanup in (("Space stop --if-mine", lambda: S.space("stop", "--if-mine")),
                              ("isolated lab down", lambda: lab("down"))):
            try:
                cleanup()
                say("[PASS] " + name)
            except (Exception, SystemExit) as exc:
                failures.append(name)
                say("[FAIL] " + name + " -- " + str(exc))
        say("RESULT: " + ("FAIL" if failures else "PASS"))
        pathlib.Path(S.OUT).parent.mkdir(parents=True, exist_ok=True)
        pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)
