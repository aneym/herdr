"""Private JSON-lines file protocol, bootstrapped over SSH stdin (Python 3.8)."""
import base64
import json
import os
import stat
import sys
import re
import subprocess
import time
import threading
import queue
import urllib.request
import urllib.parse

HOME = os.path.expanduser("~")
ROOTS = [os.path.realpath(os.path.join(HOME, suffix)) for suffix in
         (".claude/projects", ".codex/sessions", ".agent-rails")]


def allowed_path(path):
    if not isinstance(path, str) or not (os.path.isabs(path) or path.startswith("~/")):
        raise ValueError("path not allowed")
    path = os.path.realpath(os.path.expanduser(path))
    if not any(path.startswith(root + os.sep) for root in ROOTS):
        raise ValueError("path not allowed")
    return path


ROOT_FDS = {}


def open_beneath(path, flags):
    """Opens a canonical allowed path one component at a time from its root, never following a
    symlink, so a component swapped for one after allowed_path cannot lead outside the roots."""
    root = next(root for root in ROOTS if path.startswith(root + os.sep))
    if root not in ROOT_FDS:
        # Held for the session: a root renamed or replaced later cannot redirect the walk.
        ROOT_FDS[root] = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
    fd = os.dup(ROOT_FDS[root])
    try:
        parts = path[len(root) + 1:].split(os.sep)
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        return os.open(parts[-1], flags | os.O_NOFOLLOW, dir_fd=fd)
    finally:
        os.close(fd)


def metadata(info):
    return {"size": info.st_size, "mtime_ms": max(0, info.st_mtime_ns // 1000000),
            "inode": info.st_ino}


FACTORY_CAP = 1048576
FACTORY_CACHE = {}


def factory_cached(key, interval, load):
    now = time.monotonic()
    previous = FACTORY_CACHE.get(key)
    if previous is not None and now - previous[0] < interval:
        return previous[1]
    try:
        value = load()
    except (OSError, ValueError):
        value = None
    FACTORY_CACHE[key] = (now, value)
    return value


def factory_pools_read(url):
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme not in ("http", "https") or parsed.hostname not in ("127.0.0.1", "localhost") or parsed.username or parsed.password:
        raise ValueError("pools URL not allowed")
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            raise ValueError("pools redirect not allowed")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open(urllib.request.Request(url, method="GET"), timeout=4) as response:
        data = response.read(FACTORY_CAP + 1)
    if len(data) > FACTORY_CAP:
        raise ValueError("pools response too large")
    return json.loads(data)


def factory_pools(url):
    # A socket timeout alone resets per read; bound the entire GET, including headers.
    result = queue.Queue(maxsize=1)
    def fetch():
        try:
            result.put((factory_pools_read(url), None))
        except Exception as error:
            result.put((None, error))
    threading.Thread(target=fetch, daemon=True).start()
    try:
        value, error = result.get(timeout=4)
    except queue.Empty:
        raise TimeoutError("pools request timed out")
    if error:
        raise error
    return value


def factory_file(path):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
        with os.fdopen(fd, "rb") as source:
            if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
                return None
            data = source.read(FACTORY_CAP + 1)
        return json.loads(data) if len(data) <= FACTORY_CAP else None
    except (OSError, ValueError):
        return None


def factory_exec(argv):
    try:
        result = subprocess.run(argv, capture_output=True, text=True, timeout=5)
        return (result.stdout.strip() or result.stderr.strip()) if result.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        return None


def factory_alive(pid):
    if type(pid) is not int or pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def factory_bundle():
    # Fixed host-owned inputs only; client fields are never consulted.
    def source(key, default):
        return os.path.expanduser(os.environ.get(key, "").strip() or default)
    paths = {
        "overlay": source("FACTORY_OVERLAY", HOME + "/.agent-rails/herdr/overlay.json"),
        "boxes": source("FACTORY_BOXES", HOME + "/.agent-rails/factory/boxes.json"),
        "poolState": source("FACTORY_POOLSTATE", HOME + "/.agent-rails/factory/state/pool.json"),
        "disk": source("FACTORY_DISK", HOME + "/.agent-rails/fleet-disk-watch/state.json"),
        "routing": source("FACTORY_ROUTING", HOME + "/.agent-lb/managed/coding-agents/routing-table.json"),
        "decider": "/Volumes/StudioExt/repos/agent-lb/clients/open-factory/open_factory/decider.json",
    }
    bundle = {key: factory_cached(key, 1 if key == "overlay" else 15, lambda path=path: factory_file(path)) for key, path in paths.items()}
    directory = source("FACTORY_WORKFLOWS_DIR", HOME + "/.agent-rails/workflows/tabs")
    def read_flights():
        try:
            flights = []
            fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NONBLOCK)
            try:
                names = os.listdir(fd)
            finally:
                os.close(fd)
            for name in names:
                if not name.endswith(".json"):
                    continue
                row = factory_file(os.path.join(directory, name))
                if isinstance(row, dict) and (factory_alive(row.get("runner_pid")) or factory_alive(row.get("child_pid"))):
                    flights.append(dict(row, run_id=row.get("run_id") or name))
            return flights
        except OSError:
            return None
    bundle["flights"] = factory_cached("flights", 5, read_flights)
    repo = source("FACTORY_REPO", "/Volumes/StudioExt/repos/agent-rails")
    bundle["landed"] = factory_cached("landed", 60, lambda: factory_exec(["/usr/bin/git", "--no-pager", "-C", repo, "log", "origin/main", "--since=midnight", "--format=%h%x09%ct%x09%s"]))
    try:
        interval = float(os.environ.get("FACTORY_POOLS_INTERVAL", "60"))
        if not 0 < interval < float("inf"):
            interval = 60
    except ValueError:
        interval = 60
    bundle["pools"] = factory_cached("pools", interval, lambda: factory_pools(source("FACTORY_POOLS_URL", "http://127.0.0.1:2455/api/pools")))
    bundle["poolsAgeSeconds"] = max(0, time.monotonic() - FACTORY_CACHE["pools"][0])
    bundle["poolsInterval"] = interval
    return bundle


def request(req):
    op = req.get("op")
    if op == "factory_route_pick":
        route = req.get("route")
        if route not in ("implement", "mechanical"):
            raise ValueError("invalid factory route")
        try:
            result = subprocess.run([HOME + "/.local/bin/route", "pick", route], capture_output=True, text=True, timeout=5)
            return {"text": result.stdout.strip() or result.stderr.strip()}
        except (OSError, subprocess.TimeoutExpired):
            return {"text": "route pick failed"}
    if op == "factory":
        return factory_bundle()
    if op == "action":
        verb, args = req.get("verb"), req.get("args")
        if verb not in ("park", "unpark", "approve") or not isinstance(args, list) or not args:
            raise ValueError("invalid action")
        if not all(isinstance(a, str) and chr(0) not in a for a in args):
            raise ValueError("invalid arguments")
        if not re.fullmatch(r"[A-Za-z0-9._:][A-Za-z0-9._:-]*", args[0]):
            raise ValueError("invalid target")
        if verb == "approve":
            if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,80}", args[0]) or len(args) != 3 or not args[1].startswith("--quote=") or not args[1][8:].strip() or args[2] != "--by=alex":
                raise ValueError("invalid approval")
        else:
            if len(args) > 2 or (len(args) == 2 and (verb != "park" or not args[1].startswith("--note="))):
                raise ValueError("invalid park arguments")
            by = req.get("by")
            if not isinstance(by, str) or not re.fullmatch(r"herdr-shell@[a-z0-9_-]+", by):
                raise ValueError("invalid attribution")
            args = [args[0], "--by=" + by] + args[1:]
        result = subprocess.run(["python3", os.path.join(HOME, ".local/bin/herdr-shell-remote"), verb] + args,
                                capture_output=True, text=True, timeout=18)
        reply = json.loads(result.stdout)
        if result.returncode or reply.get("ok") is not True:
            raise ValueError(reply.get("error", "action failed"))
        return reply
    if op == "home":
        return {"home": HOME}
    if op == "list":
        # Entry names only, for card folders such as ~/.agent-rails/agents. As in read, the
        # listing goes through the directory opened beneath its root, checked to still be the allowed one.
        path = allowed_path(req.get("path"))
        try:
            fd = open_beneath(path, os.O_RDONLY | os.O_DIRECTORY)
        except FileNotFoundError:
            return {"names": []}
        try:
            info = os.fstat(fd)
            realpath = allowed_path(path)
            current = os.stat(realpath)
            if realpath != path or (info.st_dev, info.st_ino) != (current.st_dev, current.st_ino):
                raise ValueError("path not allowed")
            names = os.listdir(fd)
        finally:
            os.close(fd)
        return {"names": sorted(names)[:256]}
    if op not in ("stat", "read"):
        raise ValueError("unknown operation")
    path = allowed_path(req.get("path"))
    try:
        info = os.stat(path)
    except FileNotFoundError:
        if op == "stat":
            return {"exists": False, "size": 0, "mtime_ms": 0, "inode": 0}
        raise
    if not stat.S_ISREG(info.st_mode) or info.st_nlink > 1:
        raise ValueError("path not allowed")
    if op == "stat":
        return dict(metadata(info), exists=True)
    offset = req.get("offset", 0)
    maximum = req.get("max", 0)
    if (type(offset) is not int or not 0 <= offset <= 2**64 - 1 or
            type(maximum) is not int or not 0 <= maximum <= 2**32 - 1):
        raise ValueError("invalid read range")
    # Nonblocking open prevents a swapped-in FIFO from hanging the helper.
    fd = open_beneath(path, os.O_RDONLY | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as file:
        info = os.fstat(file.fileno())
        realpath = allowed_path(path)
        current = os.stat(realpath)
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink > 1 or
                realpath != path or
                (info.st_dev, info.st_ino) != (current.st_dev, current.st_ino)):
            raise ValueError("path not allowed")
        data = b""
        if offset < info.st_size:
            file.seek(offset)
            data = file.read(min(maximum, 2097152, info.st_size - offset))
    return dict(metadata(info), offset=offset,
                data_b64=base64.b64encode(data).decode("ascii"))


for line in sys.stdin.buffer:
    req = {}
    try:
        req = json.loads(line)
        result = dict(request(req), id=req.get("id"), ok=True)
    except Exception as error:
        result = {"id": req.get("id") if isinstance(req, dict) else None,
                  "ok": False, "error": str(error)}
    print(json.dumps(result, separators=(",", ":")), flush=True)
