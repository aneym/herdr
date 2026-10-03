#!/usr/bin/env python3
"""herdr-shell-space: run agent copies of Herdr Shell inside a Cua Space (a macOS VM
on Studio), never on Alex's desktop. Alex, 2026-10-02 ~16:55 ET: "have the herdr shell
app cutover to using cua spaces immediately, so it doesnt take up my current workspace."

  herdr-shell-space up                      VM running, ssh key in, herdr binary current
  herdr-shell-space start [--app PATH] [--socket HOST_SOCK] [--live] [-- APP ARGS...]
        Bridges HOST_SOCK into the guest over ssh (-R unix socket), copies the app in
        and launches it there. --app takes a .app bundle (default: the installed prod
        ~/Applications/Herdr Shell.app) or a bare dev binary (.build/release/HerdrShell).
        Without --socket the app gets a lab socket only if you name one; --live bridges
        Studio's live server (~/.config/herdr/herdr.sock). Never type into a live
        bridge: keys reach real agent panes.
  herdr-shell-space node                    install the guest Node 22 runtime once
  herdr-shell-space pull GUEST LOCAL        pull guest state or a screenshot
  herdr-shell-space fifo JSON               send a command to the guest control FIFO
  start also accepts --env K=V and --push LOCAL=GUEST (repeatable).

  herdr-shell-space shot OUT.png            screenshot of the Space desktop
  herdr-shell-space driver TOOL [JSON] [TOOL [JSON] ...]
        cua-driver tools inside the Space (list_windows, get_window_state, click,
        type_text, press_key, ...), all in one MCP session: a pixel click needs a
        get_window_state snapshot taken earlier in the same call.
  herdr-shell-space exec CMD...             a shell command inside the Space (sh -c)
  herdr-shell-space status                  VM, bridge, app pids (JSON)
  herdr-shell-space stop                    quit the app in the Space, drop the bridge
  herdr-shell-space down                    stop + power the VM off (disk kept)

The Space is `herdr-qa` (ghcr.io/trycua/macos:26-slim through Lume, storage and pull
temp on /Volumes/StudioExt/cua, see lume-serve-ext). State: ~/.cua/herdr-space/.
Herdr notes: ~/.config/herdr/CUSTOMIZATIONS.md "Herdr Shell in a Cua Space".
"""
import hashlib, json, os, re, shlex, signal, stat, subprocess, sys, time, urllib.request

SPACE = os.environ.get("HERDR_SPACE", "herdr-qa")
IMAGE = "macos:26-slim"
STATE = os.path.expanduser("~/.cua/herdr-space")
KEY = os.path.join(STATE, "id_ed25519")
BRIDGE_PID = os.path.join(STATE, "bridge.pid")
LIVE_SOCK = os.path.expanduser("~/.config/herdr/herdr.sock")
HERDR_BIN = os.path.expanduser("~/.local/bin/herdr")
PROD_APP = os.path.expanduser("~/Applications/Herdr Shell.app")
GHOSTTY_RES = "/Applications/Ghostty.app/Contents/Resources/ghostty"
GUEST_USER = "lume"
GUEST_HOME = "/Users/" + GUEST_USER
GUEST_SOCK = GUEST_HOME + "/.config/herdr/herdr.sock"  # prod bundles: where the prod app looks by default
GUEST_LAB_SOCK = GUEST_HOME + "/.herdr-space/herdr.sock"  # dev builds: they refuse any */.config/herdr/herdr.sock
GUEST_HERDR = GUEST_HOME + "/.local/bin/herdr"
GUEST_APPS = GUEST_HOME + "/Applications"
GUEST_RES = GUEST_HOME + "/.herdr-space/ghostty"
NOPROXY = {k: v for k, v in os.environ.items() if k.lower() not in ("http_proxy", "https_proxy", "all_proxy")}
# The cua binary (Developer ID, hardened) is refused macOS Local Network access from agent
# shells ("No route to host" to 192.168.64.x), while Apple tools and Homebrew python are
# not. So spacesd is reached through a loopback forwarder (python) and registered as a
# direct Space. The VM's spacesd token is the one `cua spaces create` wrote; it goes to
# cua through CUA_ENV_TOKEN, never argv.
FWD_PORT = 13211
REF = f"direct:127.0.0.1:{FWD_PORT}"
TOKEN_FILE = os.path.expanduser(f"~/.cua/vmm/lume/{SPACE}/setup/env-token")
FWD_PID = os.path.join(STATE, "fwd.json")
LUME_API = "http://127.0.0.1:7777/lume"
PY = "/opt/homebrew/bin/python3"


def die(msg, code=1):
    print("herdr-shell-space: " + msg, file=sys.stderr)
    sys.exit(code)


def cua_env():
    env = dict(NOPROXY)
    if os.path.exists(TOKEN_FILE):
        env["CUA_ENV_TOKEN"] = open(TOKEN_FILE).read().strip()
    return env


def run(argv, check=True, capture=True, timeout=600):
    env = cua_env() if argv[0] == "cua" else NOPROXY
    r = subprocess.run(argv, capture_output=capture, text=True, env=env, timeout=timeout)
    if check and r.returncode != 0:
        die(f"{' '.join(shlex.quote(a) for a in argv)} failed ({r.returncode}): {(r.stderr or r.stdout or '').strip()[-800:]}")
    return r


def gexec(cmd, check=True, timeout=300):
    """Run `sh -c cmd` in the guest as the desktop user (through cua-spacesd)."""
    return run(["cua", "sb", "exec", REF, cmd], check=check, timeout=timeout)


def lume_vm():
    try:
        with urllib.request.urlopen(f"{LUME_API}/vms/{SPACE}", timeout=10) as r:
            return json.load(r)
    except Exception:
        return None


def lume_post(path, body=None):
    req = urllib.request.Request(f"{LUME_API}/vms/{SPACE}/{path}", data=json.dumps(body or {}).encode(),
                                 headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


FWD_CODE = r'''
import asyncio, sys
ip, port = sys.argv[1], int(sys.argv[2])
async def pipe(r, w):
    try:
        while (d := await r.read(65536)):
            w.write(d); await w.drain()
    except Exception:
        pass
    finally:
        try: w.close()
        except Exception: pass
async def handle(r, w):
    try:
        r2, w2 = await asyncio.open_connection(ip, 3211)
    except Exception:
        w.close(); return
    await asyncio.gather(pipe(r, w2), pipe(r2, w))
async def main():
    srv = await asyncio.start_server(handle, "127.0.0.1", port)
    await srv.serve_forever()
asyncio.run(main())
'''


def fwd(guest_ip):
    """Loopback 127.0.0.1:FWD_PORT -> guest_ip:3211, restarted when the VM's address changes."""
    try:
        cur = json.load(open(FWD_PID))
        os.kill(cur["pid"], 0)
        if cur["ip"] == guest_ip:
            return
        os.kill(cur["pid"], signal.SIGTERM)
        time.sleep(0.3)
    except (OSError, ValueError, KeyError):
        pass
    p = subprocess.Popen([PY, "-c", FWD_CODE, guest_ip, str(FWD_PORT)], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    json.dump({"pid": p.pid, "ip": guest_ip}, open(FWD_PID, "w"))
    time.sleep(0.5)


def register():
    r = run(["cua", "spaces", "ls", "--json"], check=False)
    if REF in (r.stdout or ""):
        return
    run(["cua", "spaces", "add", f"127.0.0.1:{FWD_PORT}", "--name", SPACE])


def ip():
    vm = lume_vm() or {}
    return vm.get("ipAddress") or vm.get("ip_address") or ""


def ssh_base():
    return ["ssh", "-i", KEY, "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes", "-o", "ControlMaster=no", "-o", "ControlPath=none",
            "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=" + os.path.join(STATE, "known_hosts"),
            "-o", "ConnectTimeout=8", f"{GUEST_USER}@{ip()}"]


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def push(local, guest):
    gexec(f"mkdir -p {shlex.quote(os.path.dirname(guest))}")
    run(["cua", "sb", "cp", local, f"{REF}:{guest}"], timeout=900)
    gexec(f"chmod {stat.S_IMODE(os.stat(local).st_mode):o} {shlex.quote(guest)}")


def push_tree(local_dir, guest_dir):
    """Copy a directory (an .app bundle) in as one tarball, keeping symlinks and modes."""
    tgz = os.path.join(STATE, "push.tgz")
    run(["tar", "-czf", tgz, "-C", local_dir, "."], timeout=900)
    push(tgz, "/tmp/herdr-space-push.tgz")
    gexec(f"rm -rf {shlex.quote(guest_dir)} && mkdir -p {shlex.quote(guest_dir)} && "
          f"tar -xzf /tmp/herdr-space-push.tgz -C {shlex.quote(guest_dir)} && "
          f"rm -f /tmp/herdr-space-push.tgz")
    os.unlink(tgz)


def pull(guest, local):
    os.makedirs(os.path.dirname(os.path.abspath(local)), exist_ok=True)
    run(["cua", "sb", "cp", f"{REF}:{guest}", local], timeout=900)


def node():
    version = "v22.20.0"
    root = GUEST_HOME + "/.herdr-space/node"
    if gexec(f"cat {root}/version 2>/dev/null", check=False).stdout.strip() == version:
        print(root + "/bin/node")
        return
    archive = os.path.join(STATE, "node-" + version + ".tgz")
    url = f"https://nodejs.org/dist/{version}/node-{version}-darwin-arm64.tar.gz"
    run(["curl", "--fail", "--location", "--output", archive, url], timeout=900)
    guest_archive = GUEST_HOME + "/.herdr-space/node.tgz"
    push(archive, guest_archive)
    gexec(f"mkdir -p {root} && tar -xzf {guest_archive} --strip-components=1 -C {root} && "
          f"{root}/bin/node --version > {root}/version && rm -f {guest_archive}")
    print(root + "/bin/node")


def fifo(body):
    doc = json.load(open(os.path.join(STATE, "app.json")))
    control = doc.get("control")
    if not control:
        die("app was not started with --control")
    line = json.dumps(json.loads(body)) + "\n"
    gexec(f"test -p {shlex.quote(control)} && printf %s {shlex.quote(line)} > {shlex.quote(control)}", timeout=15)


def up():
    os.makedirs(STATE, mode=0o700, exist_ok=True)
    run(["lume-serve-ext"])
    vm = lume_vm()
    if vm is None:
        # Pulls (first time ~22 GB) and boots the VM, then fails its own Local Network
        # check; the VM and its token stay, and the forwarder below reaches it.
        print(f"creating Space {SPACE} ({IMAGE}, ~22 GB pull on first run)...", flush=True)
        run(["cua", "spaces", "create", IMAGE, "--name", SPACE, "--runtime", "lume",
             "--cpus", "4", "--memory-mb", "8192"], check=False, capture=False, timeout=7200)
        vm = lume_vm()
        if vm is None:
            die("create left no VM; see lume-serve.log")
    if vm.get("status") != "running":
        lume_post("run", {"noDisplay": True})
    for _ in range(60):
        vm = lume_vm() or {}
        if vm.get("status") == "running" and vm.get("ipAddress"):
            break
        time.sleep(2)
    else:
        die("VM did not come up with an address")
    fwd(vm["ipAddress"])
    register()
    for _ in range(90):
        if gexec("echo ok", check=False, timeout=30).stdout.strip() == "ok":
            break
        time.sleep(2)
    else:
        die("cua-spacesd in the Space did not answer")
    if not os.path.exists(KEY):
        run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "herdr-shell-space", "-f", KEY])
    pub = open(KEY + ".pub").read().strip()
    gexec("mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && "
          f"grep -qxF {shlex.quote(pub)} ~/.ssh/authorized_keys || echo {shlex.quote(pub)} >> ~/.ssh/authorized_keys; "
          "chmod 600 ~/.ssh/authorized_keys")
    want = sha(HERDR_BIN)
    have = gexec(f"shasum -a 256 {GUEST_HERDR} 2>/dev/null | cut -d' ' -f1", check=False).stdout.strip()
    if have != want:
        push(HERDR_BIN, GUEST_HERDR)
        gexec(f"chmod 755 {GUEST_HERDR}")
    if os.path.isdir(GHOSTTY_RES) and gexec(f"test -d {GUEST_RES}/themes && echo y", check=False).stdout.strip() != "y":
        push_tree(GHOSTTY_RES, GUEST_RES)
    print(json.dumps({"space": SPACE, "ip": ip(), "herdr_sha": want[:12]}))


def bridge_alive():
    try:
        pid = int(open(BRIDGE_PID).read())
        os.kill(pid, 0)
        return pid
    except (OSError, ValueError):
        return 0


def client_sock(api_sock):
    stem = os.path.splitext(os.path.basename(api_sock))[0]
    return os.path.join(os.path.dirname(api_sock), stem + "-client.sock")


def bridge(host_sock, guest_sock):
    stop_bridge()
    if not os.path.exists(host_sock):
        die(f"no socket at {host_sock}")
    gexec(f"mkdir -p {os.path.dirname(guest_sock)} && rm -f {guest_sock}")
    log = open(os.path.join(STATE, "bridge.log"), "a")
    # herdr derives the client socket from the api socket (<stem>-client.sock beside it);
    # bridge both so the app's API calls and `herdr terminal attach` both reach the server.
    fwds = ["-R", f"{guest_sock}:{host_sock}"]
    host_client = client_sock(host_sock)
    if os.path.exists(host_client):
        fwds += ["-R", f"{client_sock(guest_sock)}:{host_client}"]
        gexec(f"rm -f {client_sock(guest_sock)}")
    p = subprocess.Popen(ssh_base()[:-1] + ["-N", "-o", "ExitOnForwardFailure=yes", "-o", "ServerAliveInterval=15"]
                         + fwds + [ssh_base()[-1]],
                         stdin=subprocess.DEVNULL, stdout=log, stderr=log, env=NOPROXY, start_new_session=True)
    open(BRIDGE_PID, "w").write(str(p.pid))
    for _ in range(40):
        if gexec(f"test -S {guest_sock} && echo y", check=False).stdout.strip() == "y":
            return
        if p.poll() is not None:
            die("ssh bridge exited; see " + os.path.join(STATE, "bridge.log"))
        time.sleep(0.5)
    die("bridge socket never appeared in the guest")


def stop_bridge():
    pid = bridge_alive()
    if pid:
        os.kill(pid, signal.SIGTERM)
    if os.path.exists(BRIDGE_PID):
        os.unlink(BRIDGE_PID)


def start(argv):
    app, sock, live, extra = PROD_APP, None, False, []
    envs, pushes = [], []
    if "--" in argv:
        i = argv.index("--")
        argv, extra = argv[:i], argv[i + 1:]
    it = iter(argv)
    for a in it:
        if a == "--app":
            app = os.path.abspath(os.path.expanduser(next(it)))
        elif a == "--socket":
            sock = os.path.abspath(os.path.expanduser(next(it)))
        elif a == "--env":
            value = next(it)
            if "=" not in value or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", value.split("=", 1)[0]):
                die("--env requires K=V", 2)
            envs.append(value)
        elif a == "--push":
            pushes.append(next(it).split("=", 1))
        elif a == "--live":
            live = True
        else:
            die("unknown arg " + a, 2)
    if live:
        sock = LIVE_SOCK
    if not sock:
        die("name the herdr server: --socket <lab socket> or --live (Studio's live server)", 2)
    bundle = app.endswith(".app")
    if live and not bundle:
        die("dev builds refuse the live server; use a lab socket, or the prod bundle with --live", 2)
    guest_sock = GUEST_SOCK if bundle and live else GUEST_LAB_SOCK
    up()
    bridge(sock, guest_sock)
    stop_app()
    for local, guest in pushes:
        (push_tree if os.path.isdir(local) else push)(local, guest)
    res = ["--ghostty-resources", GUEST_RES]
    if bundle:
        guest_app = f"{GUEST_APPS}/{os.path.basename(app)}"
        push_tree(app, guest_app)
        exe = f"{guest_app}/Contents/MacOS/" + os.listdir(os.path.join(app, "Contents", "MacOS"))[0]
    else:
        exe = GUEST_HOME + "/.herdr-space/bin/" + os.path.basename(app)
        push(app, exe)
        gexec(f"chmod 755 {shlex.quote(exe)}")
    args = ["--herdr", GUEST_HERDR, "--socket", guest_sock] + res + extra
    cmd = (f"cd ~; nohup env {' '.join(shlex.quote(e) for e in envs)} {shlex.quote(exe)} {' '.join(shlex.quote(a) for a in args)} "
           f"> ~/.herdr-space/app.log 2>&1 < /dev/null & echo $!")
    gexec("mkdir -p ~/.herdr-space")
    pid = gexec(cmd).stdout.strip().splitlines()[-1]
    open(os.path.join(STATE, "app.json"), "w").write(json.dumps({"exe": exe, "pid": pid, "socket": sock, "guest_socket": guest_sock,
        "control": extra[extra.index("--control") + 1] if "--control" in extra else None}))
    time.sleep(3)
    alive = gexec(f"kill -0 {pid} 2>/dev/null && echo y", check=False).stdout.strip() == "y"
    if not alive:
        die("app exited at once; log:\n" + gexec("tail -20 ~/.herdr-space/app.log", check=False).stdout)
    print(json.dumps({"space": SPACE, "app_pid": pid, "exe": exe, "host_socket": sock, "guest_socket": guest_sock}))


def stop_app():
    try:
        exe = json.load(open(os.path.join(STATE, "app.json")))["exe"]
    except (OSError, ValueError, KeyError):
        return
    gexec(f"pkill -f {shlex.quote(exe)} || true", check=False)


def driver(calls):
    """Call cua-driver tools in the Space over ONE MCP session (spacesd /mcp), so a
    get_window_state snapshot and the pixel click that needs it share a session.
    Prints each result's text and structuredContent (images dropped) as JSON lines."""
    url = f"http://127.0.0.1:{FWD_PORT}/mcp"
    tok = cua_env().get("CUA_ENV_TOKEN", "")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    sid = None

    def rpc(method, params, rid):
        h = {"authorization": "Bearer " + tok, "content-type": "application/json",
             "accept": "application/json, text/event-stream"}
        if sid:
            h["mcp-session-id"] = sid
        body = {"jsonrpc": "2.0", "method": method, "params": params}
        if rid is not None:
            body["id"] = rid
        req = urllib.request.Request(url, data=json.dumps(body).encode(), headers=h, method="POST")
        with opener.open(req, timeout=120) as r:
            raw, new_sid = r.read().decode(), r.headers.get("mcp-session-id")
        if raw.startswith("event:") or raw.startswith("data:"):
            raw = "".join(l[5:] for l in raw.splitlines() if l.startswith("data:"))
        return (json.loads(raw) if raw.strip() else {}), new_sid

    _, sid = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                "clientInfo": {"name": "herdr-shell-space", "version": "1"}}, 0)
    rpc("notifications/initialized", {}, None)
    rc = 0
    for n, (tool, args) in enumerate(calls, 1):
        res, _ = rpc("tools/call", {"name": tool, "arguments": args}, n)
        r = res.get("result") or {}
        if res.get("error") or r.get("isError"):
            rc = 1
        text = " ".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
        out = {"tool": tool, "text": text[:2000], "structured": r.get("structuredContent"), "error": res.get("error")}
        line = json.dumps(out)
        if len(line) > 50000:
            out["structured"] = {"truncated_bytes": len(line)}
            line = json.dumps(out)
        print(line)
    return rc


def status():
    vm = lume_vm() or {}
    out = {"space": SPACE, "vm": vm.get("status"), "ip": vm.get("ipAddress"), "bridge_pid": bridge_alive()}
    try:
        out["app"] = json.load(open(os.path.join(STATE, "app.json")))
        if vm.get("status") == "running":
            out["app"]["alive"] = gexec(f"kill -0 {out['app']['pid']} 2>/dev/null && echo y", check=False).stdout.strip() == "y"
    except (OSError, ValueError):
        pass
    print(json.dumps(out))


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    cmd, rest = sys.argv[1], sys.argv[2:]
    if cmd == "up":
        up()
    elif cmd == "start":
        start(rest)
    elif cmd == "pull":
        pull(*rest)
    elif cmd == "node":
        node()
    elif cmd == "fifo":
        fifo(rest[0])
    elif cmd == "shot":
        out = os.path.abspath(rest[0] if rest else "herdr-space.png")
        run(["cua", "sb", "screenshot", REF, "-o", out])
        print(out)
    elif cmd == "driver":
        if not rest:
            die("driver TOOL [JSON] [TOOL [JSON] ...]", 2)
        calls, i = [], 0
        while i < len(rest):
            tool, arg = rest[i], {}
            if i + 1 < len(rest) and rest[i + 1].lstrip().startswith("{"):
                arg = json.loads(rest[i + 1])
                i += 1
            calls.append((tool, arg))
            i += 1
        sys.exit(driver(calls))
    elif cmd == "exec":
        r = gexec(" ".join(rest), check=False)
        sys.stdout.write(r.stdout)
        sys.stderr.write(r.stderr)
        sys.exit(r.returncode)
    elif cmd == "status":
        status()
    elif cmd == "stop":
        stop_app()
        stop_bridge()
        print("stopped")
    elif cmd == "down":
        stop_app()
        stop_bridge()
        try:
            lume_post("stop")
        except Exception as e:
            print(f"lume stop: {e}", file=sys.stderr)
        print("down")
    else:
        die("unknown command " + cmd, 2)


if __name__ == "__main__":
    main()
