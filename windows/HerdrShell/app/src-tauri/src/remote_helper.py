"""Private JSON-lines file protocol, bootstrapped over SSH stdin (Python 3.8)."""
import base64
import json
import os
import stat
import sys
import re
import subprocess

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


def request(req):
    op = req.get("op")
    if op == "action":
        verb, args = req.get("verb"), req.get("args")
        if verb not in ("park", "unpark", "approve") or not isinstance(args, list) or not args:
            raise ValueError("invalid action")
        if not all(isinstance(a, str) and chr(0) not in a for a in args):
            raise ValueError("invalid arguments")
        if not re.fullmatch(r"[A-Za-z0-9._:-]+", args[0]):
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
