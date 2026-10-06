#!/usr/bin/env python3
"""app.py start|stop|cmd JSON  - run the HerdrShell app detached and drive its FIFO hook."""
import json, os, signal, subprocess, sys, time
D = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LAB = os.path.expanduser("~/.cache/herdr-build/" + os.environ.get("SHELL_LAB", "shellspike"))
FIFO = os.path.join(LAB, "control.fifo")
BIN = os.environ.get("HERDR_SHELL_APP") or os.path.join(D, ".build", "release", "HerdrShell")

def pids():
    r = subprocess.run(["pgrep", "-f", BIN], capture_output=True, text=True)
    return [int(p) for p in r.stdout.split()]

def start(extra=()):
    if os.environ.get("HERDR_SHELL_SPACE") == "1":
        import scenario
        print(scenario.app("start", *extra))
        return
    if "--host-ok" not in extra:
        raise SystemExit("start requires HERDR_SHELL_SPACE=1 or explicit --host-ok")
    extra = tuple(a for a in extra if a != "--host-ok")
    extra = ("--agent-run",) + tuple(a for a in extra if a != "--agent-run")
    t0 = time.time()
    if os.path.exists(FIFO):
        os.unlink(FIFO)  # else a writer can open the old inode before the app replaces it
    pid = os.fork()
    if pid == 0:
        os.setsid()
        if os.fork() == 0:
            nul = os.open(os.devnull, os.O_RDWR)
            for fd in (0, 1, 2): os.dup2(nul, fd)
            os.execv("/bin/bash", ["/bin/bash", os.path.join(D, "scripts", "run.sh"), *extra])
        os._exit(0)
    os.waitpid(pid, 0)
    for _ in range(100):
        if os.path.exists(FIFO) and pids(): break
        time.sleep(0.05)
    print(json.dumps({"pids": pids(), "fifo_ready_s": round(time.time() - t0, 2)}))

def stop():
    if os.environ.get("HERDR_SHELL_SPACE") == "1":
        import scenario
        print(scenario.app("stop"))
        return
    for p in pids(): os.kill(p, signal.SIGTERM)

def cmd(obj):
    if os.environ.get("HERDR_SHELL_SPACE") == "1":
        import scenario
        scenario.cmd(obj)
        return
    with open(FIFO, "w") as f: f.write(json.dumps(obj) + "\n")

if __name__ == "__main__":
    if sys.argv[1] == "start":
        start(sys.argv[2:])  # extra args go to the app, e.g. --appearance light
    else:
        {"stop": stop}.get(sys.argv[1], lambda: cmd(json.loads(sys.argv[2])))()
