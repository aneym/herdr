"""Integration owner for the helper wire and path-security contracts.

Real pipes and files catch offset/cap mistakes and traversal/symlink/non-file
acceptance. No prior helper coverage exists; no production test seam is needed.
"""
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

BOOTSTRAP = ('import sys;n=int(sys.stdin.buffer.readline());'
             'exec(compile(sys.stdin.buffer.read(n),"herdr-shell-helper","exec"))')
SCRIPT = (Path(__file__).resolve().parents[1] / "src/remote_helper.py").read_bytes()


class RemoteHelperTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        for root in (".claude/projects", ".codex/sessions", ".agent-rails"):
            (self.home / root).mkdir(parents=True)
        self.process = subprocess.Popen(
            [sys.executable, "-I", "-u", "-c", BOOTSTRAP],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=dict(os.environ, HOME=str(self.home)))
        self.addCleanup(self.close_helper)
        self.process.stdin.write(str(len(SCRIPT)).encode() + b"\n" + SCRIPT)
        self.process.stdin.flush()
        self.next_id = 0

    def close_helper(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        stderr = self.process.stderr.read().decode()
        self.process.stdout.close()
        self.process.stderr.close()
        self.assertEqual(self.process.returncode, 0, stderr)
        self.assertEqual(stderr, "")

    def request(self, op, **args):
        self.next_id += 1
        self.process.stdin.write(json.dumps(dict(args, id=self.next_id, op=op)).encode() + b"\n")
        self.process.stdin.flush()
        response = json.loads(self.process.stdout.readline())
        self.assertEqual(response["id"], self.next_id)
        return response

    def test_reads_metadata_and_range_contract(self):
        self.assertEqual(self.request("home")["home"], str(self.home))
        content = bytes(range(256)) * 9000
        path = self.home / ".claude/projects/transcript.jsonl"
        path.write_bytes(content)
        expected_stat = path.stat()
        response = self.request("stat", path=str(path))
        self.assertTrue(response["ok"])
        self.assertTrue(response["exists"])
        self.assertEqual(response["size"], len(content))
        self.assertEqual(response["inode"], expected_stat.st_ino)
        self.assertEqual(response["mtime_ms"], expected_stat.st_mtime_ns // 1000000)
        for offset, maximum, end in ((0, 23, 23), (17, 31, 48),
                                     (len(content) - 2, 100, len(content)),
                                     (len(content), 10, len(content)),
                                     (len(content) + 100, 10, len(content)),
                                     (2**64 - 1, 10, len(content)),
                                     (0, 2**32 - 1, 2097152), (0, 0, 0)):
            with self.subTest(offset=offset, maximum=maximum):
                chunk = self.request("read", path="~/.claude/projects/transcript.jsonl",
                                     offset=offset, max=maximum)
                self.assertTrue(chunk["ok"], chunk)
                self.assertEqual(chunk["offset"], offset)
                self.assertEqual(chunk["size"], len(content))
                self.assertEqual(chunk["inode"], expected_stat.st_ino)
                self.assertEqual(chunk["mtime_ms"], expected_stat.st_mtime_ns // 1000000)
                self.assertEqual(base64.b64decode(chunk["data_b64"]), content[offset:end])
        missing = self.request("stat", path=str(path.parent / "missing"))
        self.assertEqual(missing, dict(id=self.next_id, ok=True, exists=False,
                                      size=0, mtime_ms=0, inode=0))
        for root in (".codex/sessions", ".agent-rails"):
            path = self.home / root / "lane.md"
            path.write_bytes(b"lane")
            self.assertEqual(base64.b64decode(self.request("read", path=str(path),
                             offset=0, max=100)["data_b64"]), b"lane")

    def test_lists_card_folders(self):
        agents = self.home / ".agent-rails/agents"
        for name in ("recruiter", "frank", "home"):
            (agents / name).mkdir(parents=True)
        self.assertEqual(self.request("list", path="~/.agent-rails/agents")["names"],
                         ["frank", "home", "recruiter"])
        missing = self.request("list", path="~/.agent-rails/none")
        self.assertEqual(missing["names"], [])
        (agents / "frank" / "agent.json").write_bytes(b"{}")
        self.assertFalse(self.request("list", path="~/.agent-rails/agents/frank/agent.json")["ok"])
        outside = self.home / "outside"
        outside.mkdir()
        (outside / "secret").mkdir()
        (self.home / ".agent-rails/escape").symlink_to(outside)
        for path in (str(outside), "~/.agent-rails/escape", "~/.agent-rails/../outside"):
            with self.subTest(path=path):
                response = self.request("list", path=path)
                self.assertFalse(response["ok"])
                self.assertEqual(response["error"], "path not allowed")

    def test_security_boundary(self):
        outside = self.home / "private.txt"
        outside.write_bytes(b"outside fixture")
        root = self.home / ".agent-rails"
        (root / "escape").symlink_to(outside)
        os.link(outside, root / "hardlink")
        (root / "dir").mkdir()
        os.mkfifo(root / "fifo")
        sibling = self.home / ".agent-rails-other"
        sibling.mkdir()
        (sibling / "file").write_bytes(b"sibling")
        for path in (str(root / "escape"), str(root / "hardlink"), str(root / "../private.txt"),
                     str(root / "dir"), str(root / "fifo"), str(sibling / "file"),
                     ".agent-rails/file", str(outside), "~other/.agent-rails/file"):
            for op in ("read", "stat"):
                with self.subTest(path=path, op=op):
                    response = self.request(op, path=path, offset=0, max=100)
                    self.assertFalse(response["ok"])
                    self.assertEqual(response["error"], "path not allowed")
        # A rejected request must not end the long-lived protocol session.
        self.assertTrue(self.request("home")["ok"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
