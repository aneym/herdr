"""Private JSON-lines file protocol, bootstrapped over SSH stdin (Python 3.8)."""
import base64
import json
import os
import stat
import sys

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


def metadata(info):
    return {"size": info.st_size, "mtime_ms": max(0, info.st_mtime_ns // 1000000),
            "inode": info.st_ino}


def request(req):
    op = req.get("op")
    if op == "home":
        return {"home": HOME}
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
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
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
