"""Integration at the Python host boundary: fixed reads/argv and a real HTTP server."""
import http.server
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import threading
import unittest

HELPER = pathlib.Path(__file__).with_name("remote_helper.py")


class FactoryTest(unittest.TestCase):
    def test_client_fields_cannot_select_paths_or_commands(self):
        # An interpreter audit hook observes OS operations, not our collaborators.
        harness = r'''
import io, json, os, runpy, sys
reads, commands = [], []
def audit(event, args):
    if event == "open" and isinstance(args[0], str): reads.append(args[0])
    if event == "subprocess.Popen":
        commands.append(args[1])
        raise OSError("execution captured at OS boundary")
sys.addaudithook(audit)
sys.stdin = io.TextIOWrapper(io.BytesIO())
m = runpy.run_path(sys.argv[1])
r = m["request"]({"op":"factory", "path":"/client/secret", "argv":["sh","-c","bad"], "url":"https://example.com"})
print(json.dumps({"reads":reads,"commands":commands,"result":r}))
'''
        with tempfile.TemporaryDirectory() as home:
            env = dict(os.environ, HOME=home, FACTORY_POOLS_URL="https://example.com")
            for key in list(env):
                if key.startswith("FACTORY_") and key != "FACTORY_POOLS_URL":
                    del env[key]
            reply = subprocess.run([sys.executable, "-I", "-c", harness, str(HELPER)], env=env, capture_output=True, text=True, check=True)
            result = json.loads(reply.stdout)
            self.assertNotIn("/client/secret", result["reads"])
            for suffix in ("/.agent-rails/herdr/overlay.json", "/.agent-rails/factory/boxes.json", "/.agent-lb/managed/coding-agents/routing-table.json"):
                self.assertIn(home + suffix, result["reads"])
            self.assertEqual(result["commands"], [["/usr/bin/git", "--no-pager", "-C", "/Volumes/StudioExt/repos/agent-rails", "log", "origin/main", "--since=midnight", "--format=%h%x09%ct%x09%s"], [home + "/.local/bin/route", "pick", "implement"], [home + "/.local/bin/route", "pick", "mechanical"]])
            self.assertIsNone(result["result"]["pools"])
            self.assertIsNone(result["result"]["landed"])

    def test_pools_loopback_cap_and_redirect_floor(self):
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args): pass
            def do_GET(self):
                if self.path == "/redirect":
                    self.send_response(302); self.send_header("Location", "https://example.com"); self.end_headers(); return
                self.send_response(200); self.end_headers()
                try: self.wfile.write(b"x" * 1048577 if self.path == "/large" else b'{"pools":[]}')
                except (BrokenPipeError, ConnectionResetError): pass
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        try:
            harness = r'''
import io, json, runpy, sys
sys.stdin = io.TextIOWrapper(io.BytesIO())
m = runpy.run_path(sys.argv[1])
try: print(json.dumps({"value":m["factory_pools"](sys.argv[2])}))
except Exception as e: print(json.dumps({"error":str(e)}))
'''
            def fetch(url):
                r = subprocess.run([sys.executable, "-I", "-c", harness, str(HELPER), url], capture_output=True, text=True, check=True)
                return json.loads(r.stdout)
            base = "http://127.0.0.1:" + str(server.server_port)
            self.assertEqual(fetch(base)["value"], {"pools": []})
            self.assertEqual(fetch(base.replace("127.0.0.1", "localhost"))["value"], {"pools": []})
            for url in ("https://example.com", "file:///etc/passwd", "http://127.0.0.1.example.com", "http://user@localhost"):
                self.assertEqual(fetch(url)["error"], "pools URL not allowed")
            self.assertEqual(fetch(base + "/large")["error"], "pools response too large")
            self.assertEqual(fetch(base + "/redirect")["error"], "pools redirect not allowed")
        finally:
            server.shutdown(); server.server_close(); thread.join()

if __name__ == "__main__": unittest.main()
