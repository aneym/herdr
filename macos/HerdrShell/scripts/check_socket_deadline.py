#!/usr/bin/env python3
"""HerdrSocket.request's timeout bounds the whole exchange, request write included.

  python3 scripts/check_socket_deadline.py [--source path/to/HerdrSocket.swift]

A clipboard image upload (up to 16 MB of base64) is larger than the socket buffer.
When a forwarded peer accepted the connection and then stopped draining it, the
blocking write hung past the 30 s response deadline, leaving the paste pending
with no cancel and no beep (review finding, 2026-10-06). Peers here are local Unix
sockets driven by this script: one never reads, one reads and never replies, one
answers. The first two must fail by the deadline; the third must get its reply.
"""
import os
import pathlib
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "Sources/HerdrShell/HerdrSocket.swift"
if "--source" in sys.argv:
    SRC = pathlib.Path(sys.argv[sys.argv.index("--source") + 1]).resolve()
BUILD = pathlib.Path.home() / ".cache/herdr-build/socket-deadline"
BUILD.mkdir(parents=True, exist_ok=True)
DRIVER = BUILD / "socket_deadline"
subprocess.run(["swiftc", "-parse-as-library", str(SRC), str(ROOT / "scripts/socket_deadline.swift"),
                "-o", str(DRIVER)], check=True)

PAYLOAD = 8 * 1024 * 1024
TIMEOUT = 2.0
failures = []


def peer(path, mode):
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(path)
    srv.listen(4)
    held = []

    def serve():
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            held.append(conn)
            if mode == "stall-write":
                continue  # accept, never read
            buf = b""
            while b"\n" not in buf:
                chunk = conn.recv(1 << 20)
                if not chunk:
                    break
                buf += chunk
            if mode == "answer":
                conn.sendall(b'{"id":"t","result":{"ok":true}}\n')
            # stall-read: read the whole request, never reply

    threading.Thread(target=serve, daemon=True).start()
    return srv, held


def run(name, mode, want_reply):
    d = tempfile.mkdtemp(prefix="hsd-", dir="/tmp")
    path = os.path.join(d, "s")
    srv, held = peer(path, mode)
    t0 = time.time()
    try:
        out = subprocess.run([str(DRIVER), path, str(PAYLOAD), str(TIMEOUT)], capture_output=True,
                             text=True, timeout=TIMEOUT + 8)
        line = out.stdout.strip()
    except subprocess.TimeoutExpired:
        line = f"HUNG past {TIMEOUT + 8:.0f}s"
    wall = time.time() - t0
    srv.close()
    for c in held:
        c.close()
    if want_reply:
        ok = line.endswith('"ok":true}}') and wall < TIMEOUT + 2
    else:
        ok = line.endswith(" nil") and wall < TIMEOUT + 2
    print(f"[{'PASS' if ok else 'FAIL'}] {name}: {line} (wall {wall:.2f}s, timeout {TIMEOUT}s)")
    if not ok:
        failures.append(name)


run("peer accepts and never reads an 8 MB request", "stall-write", False)
run("peer reads the request and never replies", "stall-read", False)
run("peer answers", "answer", True)
if failures:
    raise SystemExit("FAIL: " + ", ".join(failures))
print("PASS socket deadline")
