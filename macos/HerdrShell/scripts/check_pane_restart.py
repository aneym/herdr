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
precondition(PaneRestart.next(code: "busy", message: "agent in pane pane_1 is Working", forced: false) == .confirm)
precondition(PaneRestart.next(code: "busy", message: "agent in pane pane_1 is Working", forced: true) == .error("agent in pane pane_1 is Working"))
for message in ["agent in pane pane_1 is Blocked", "previous restart is still completing"] {
    precondition(PaneRestart.next(code: "busy", message: message, forced: false) == .error(message))
}
for code in ["not_resumable", "no_session", "unsupported", "transport"] {
    precondition(PaneRestart.next(code: code, message: "server message", forced: false) == .error("server message"))
}
precondition(PaneRestart.next(code: "busy", message: "new wording", forced: false, reason: "working") == .confirm)
precondition(PaneRestart.next(code: "busy", message: "agent is Working", forced: false, reason: "blocked") == .error("agent is Working"))
precondition(PaneRestart.next(code: "busy", message: "new wording", forced: false, reason: "restart_pending") == .error("new wording"))
precondition(PaneRestart.message(code: "busy", fallback: "new wording", reason: "restart_pending") == "This agent is already restarting. Wait for it to finish.")
precondition(PaneRestart.message(code: "busy", fallback: "previous restart", reason: "blocked") == "This agent is blocked. Resolve its prompt before restarting.")
precondition(PaneRestart.message(code: "busy", fallback: "agent is Unknown") == "This agent can't restart right now")
precondition(PaneRestart.message(code: "busy", fallback: "agent is Working", reason: "unknown") == "This agent can't restart right now")
precondition(PaneRestart.requestKey(server: "studio", pane: "pane_1") != PaneRestart.requestKey(server: "book", pane: "pane_1"))
// Restart lifecycle is a timed state algorithm: acceptance is not startup completion.
let key = PaneRestart.requestKey(server: "studio", pane: "pane_1")
let now = Date(timeIntervalSince1970: 100)
var tracking = PaneRestart.Tracking()
tracking.request(key, now: now)
precondition(tracking.observe(key, restoreError: "start_failed", changed: true, running: false, now: now))
precondition(!tracking.observe(key, restoreError: "another failure", changed: true, running: false, now: now))
tracking.request(key, now: now)
precondition(!tracking.observe(key, restoreError: nil, changed: false, running: true, now: now))
precondition(!tracking.observe(key, restoreError: "later failure", changed: true, running: false, now: now))
tracking.request(key, now: now)
precondition(!tracking.observe(key, restoreError: "late failure", changed: true, running: false, now: now.addingTimeInterval(30)))
tracking.request(key, now: now)
tracking.clear(key)
precondition(!tracking.observe(key, restoreError: "after rejected reply", changed: true, running: false, now: now))
print("PASS pane menu responses; restart failure once; running/error/30s clear tracking")
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
                reply = {"error": {"code": code, "message": "new wording", "reason": "working"}} if code else {"result": {"ok": True, "command_summary": "resume"}}
                conn.sendall((json.dumps(reply) + "\n").encode())
    worker = threading.Thread(target=serve, daemon=True)
    worker.start()
    source = pathlib.Path(tmp) / "transport.swift"
    files = ["SpacesTree.swift", "MachineMerge.swift", "DeskModel.swift", "Snapshot.swift", "Machines.swift", "HerdrSocket.swift", "HerdrClient.swift", "PaneRestart.swift"]
    text = "import Foundation\nfunc log(_ message: String) {}\n" + "\n".join((ROOT / "Sources/HerdrShell" / name).read_text() for name in files)
    text += "\nlet commands = HerdrCommands(socketPath: " + json.dumps(path) + ")\n"
    text += '''
switch commands.restartAgent(paneId: "pane_1") {
case .failure(let code, let message, let reason): precondition(PaneRestart.next(code: code, message: message, forced: false, reason: reason) == .confirm)
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
