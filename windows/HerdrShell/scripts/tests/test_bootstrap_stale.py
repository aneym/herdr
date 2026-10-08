"""A checkout older than pc.DESKTOP_SAFE must not copy its PC helpers.

Integration at the git and ssh edge: real git repositories decide, and scp/ssh
are recorded instead of run. The regression this catches: a stale worktree's
pc.py bootstrap() putting the old flashing idle task back on the PC
(2026-10-08 pop-ups during League).
"""

import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pc_stale', SCRIPTS / 'pc.py')
pc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pc)
REAL_RUN = subprocess.run


def git_repo():
    d = Path(tempfile.mkdtemp())
    for cmd in (['init', '-q'], ['-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-q', '--allow-empty', '-m', 'x']):
        REAL_RUN(['git', '-C', str(d), *cmd], check=True, capture_output=True)
    return d


class BootstrapStaleTests(unittest.TestCase):
    def copies(self, repo):
        """(exit status, PC commands) for bootstrap() run from repo."""
        sent = []

        def run(argv, *a, **kw):
            if argv[0] in ('scp', 'ssh'):
                sent.append(argv[0])
                return subprocess.CompletedProcess(argv, 0, '', '')
            return REAL_RUN(argv, *a, **kw)

        with mock.patch.object(pc, 'REPO', repo), mock.patch.object(subprocess, 'run', run), \
                contextlib.redirect_stderr(io.StringIO()) as err:
            try:
                pc.bootstrap()
                code = 0
            except SystemExit as stop:
                code = stop.code
        return code, sent, err.getvalue()

    def test_stale_checkout_refuses_before_touching_the_pc(self):
        code, sent, err = self.copies(git_repo())
        self.assertNotEqual(code, 0)
        self.assertEqual(sent, [])
        self.assertIn('rebase origin/main', err)

    def test_current_checkout_and_installed_snapshot_copy(self):
        for repo in (pc.REPO, Path(tempfile.mkdtemp())):
            with self.subTest(repo=repo):
                code, sent, _ = self.copies(repo)
                self.assertEqual(code, 0)
                self.assertIn('scp', sent)


if __name__ == '__main__':
    unittest.main()
