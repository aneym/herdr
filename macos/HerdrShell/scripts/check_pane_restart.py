#!/usr/bin/env python3
"""Pure menu response table: availability, busy confirmation, force and errors.
Never launches or drives an app; runs production policy through Swift's interpreter.
"""
import os
import pathlib
import subprocess
import tempfile
ROOT = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="pane-restart-", dir=os.environ.get("TMPDIR")) as tmp:
    source = pathlib.Path(tmp) / "combined.swift"
    source.write_text((ROOT / "Sources/HerdrShell/PaneRestart.swift").read_text() + '''
precondition(!PaneRestart.enabled(hasAgent: false))
precondition(PaneRestart.enabled(hasAgent: true))
precondition(PaneRestart.next(code: nil, message: "", forced: false) == .done)
precondition(PaneRestart.next(code: "busy", message: "working", forced: false) == .confirm)
precondition(PaneRestart.next(code: "busy", message: "working", forced: true) == .error("working"))
for code in ["not_resumable", "no_session", "unsupported", "transport"] {
    precondition(PaneRestart.next(code: code, message: "server message", forced: false) == .error("server message"))
}
print("PASS pane menu availability; busy confirmation; forced busy and domain errors")
''')
    subprocess.run(["swift", "-module-cache-path", str(pathlib.Path(tmp) / "modules"), str(source)], check=True, timeout=120)

# Real command transport against a disposable Unix socket, not a mocked production client.
import json
import socket
import threading
with tempfile.TemporaryDirectory(prefix="restart-api-", dir=os.environ.get("TMPDIR")) as tmp:
    # Bind a relative path in the isolated scratch directory; AF_UNIX paths are short.
    path = "api.sock"
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    previous = os.getcwd()
    os.chdir(tmp)
    try:
        server.bind(path)
    finally:
        os.chdir(previous)
    server.listen(2)
    calls = []
    def serve():
        for code in ["busy", None]:
            conn, _ = server.accept()
            with conn:
                line = conn.makefile("rb").readline()
                calls.append(json.loads(line))
                reply = {"error": {"code": code, "message": "working"}} if code else {"result": {"ok": True, "command_summary": "resume"}}
                conn.sendall((json.dumps(reply) + "\n").encode())
    worker = threading.Thread(target=serve, daemon=True)
    worker.start()
    source = pathlib.Path(tmp) / "transport.swift"
    files = ["SpacesTree.swift", "MachineMerge.swift", "DeskModel.swift", "Snapshot.swift", "Machines.swift", "HerdrSocket.swift", "HerdrClient.swift", "PaneRestart.swift"]
    text = "import Foundation\nfunc log(_ message: String) {}\n" + "\n".join((ROOT / "Sources/HerdrShell" / name).read_text() for name in files)
    text += "\nlet commands = HerdrCommands(socketPath: " + json.dumps(path) + ")\n"
    text += '''
switch commands.restartAgent(paneId: "pane_1") {
case .failure(let code, let message): precondition(PaneRestart.next(code: code, message: message, forced: false) == .confirm)
case .success: preconditionFailure("busy must confirm")
}
switch commands.restartAgent(paneId: "pane_1", force: true) {
case .success: break
case .failure: preconditionFailure("forced restart failed")
}
print("PASS real command transport: busy then force")
'''
    source.write_text(text)
    try:
        subprocess.run(["swift", "-module-cache-path", str(pathlib.Path(tmp) / "modules"), str(source)], cwd=tmp, check=True, timeout=120)
        worker.join(timeout=5)
        assert [call["method"] for call in calls] == ["agent.restart", "agent.restart"]
        assert [call["params"] for call in calls] == [{"pane_id": "pane_1"}, {"pane_id": "pane_1", "force": True}]
    finally:
        server.close()
        (pathlib.Path(tmp) / path).unlink(missing_ok=True)
