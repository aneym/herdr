"""Off-PC Windows Shell builds reach the PC only whole, and a wrong install rolls back.

Integration at the gh, scp and ssh edge: the real pc.py runs against a fake GitHub
(run list, run view, artifact download into a real directory) and a fake PC that
records every remote command. Regressions caught: trusting an artifact without its
build stamp or with a partial checksum list, copying straight over the final name,
keeping a build that reports the wrong commit, skipping the commit check without
--relaunch, and redispatching a sha whose build keeps failing (Codex review of
3d26a4f0, 2026-10-08).
"""

import argparse
import base64
import contextlib
import hashlib
import importlib.util
import io
import itertools
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pc_artifact', SCRIPTS / 'pc.py')
pc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pc)

SHA = '1' * 40
EXE, SETUP = b'new shell exe', b'new shell installer'


def digest(data):
    return hashlib.sha256(data).hexdigest().upper()


class Fake:
    """GitHub and the PC. `files` is what the artifact download writes; `runs` the
    windows-shell.yml runs gh lists; `commit` what the relaunched app reports."""

    def __init__(self, files=None, runs=None, artifact=True, commit=SHA, pc_hash_ok=True):
        self.files = files if files is not None else good_files()
        self.runs = runs or []
        self.artifact = artifact
        self.commit = commit
        self.pc_hash_ok = pc_hash_ok
        self.dispatched = 0
        self.scp = []      # remote destinations
        self.remote = []   # decoded PowerShell scripts and helper names, in order
        self.helpers = []  # (helper, args) through gated.ps1

    def run(self, argv, **_kw):
        ok = lambda out='': subprocess.CompletedProcess(argv, 0, out, '')
        if argv[0] == 'git':
            return ok()
        if argv[0] == 'gh':
            return self.gh(argv[1:])
        if argv[0] == 'scp':
            self.scp.append(argv[-1])
            return ok()
        cmd = argv[2]
        enc = re.search(r'-EncodedCommand (\S+)', cmd)
        if enc:
            script = base64.b64decode(enc.group(1)).decode('utf-16-le')
            self.remote.append(script)
            if 'Get-FileHash' in script and not self.pc_hash_ok:
                return subprocess.CompletedProcess(argv, 9, '', '')
            return ok()
        gated = re.search(r'-Script (\S+) -ArgsB64 (\S+)', cmd)
        if gated:
            helper, args = gated.group(1), json.loads(base64.b64decode(gated.group(2)))
            self.helpers.append((helper, args))
            if helper == 'ctl.ps1':
                c = json.loads(base64.b64decode(args[args.index('-JsonB64') + 1]))
                if c['cmd'] == 'ping':
                    return ok(json.dumps({'ok': True, 'commit': self.commit}))
                return ok(json.dumps({'ok': True, 'machine': {'state': 'up'}, 'rows': [], 'panes': []}))
            return ok('done')
        return ok('{"game":false,"procs":[]}')

    def gh(self, args):
        ok = lambda out='': subprocess.CompletedProcess(['gh'], 0, out, '')
        if args[0] == 'api':
            return ok('4242' if self.artifact else '')
        if args[:2] == ['run', 'list']:
            return ok(json.dumps(self.runs))
        if args[:2] == ['run', 'view']:
            run = next(r for r in self.runs if str(r['databaseId']) == args[2])
            return ok(json.dumps({'status': 'completed', 'conclusion': run['conclusion']}))
        if args[:2] == ['workflow', 'run']:
            self.dispatched += 1
            self.runs.insert(0, {'databaseId': 900 + self.dispatched, 'status': 'completed',
                                 'conclusion': 'success', 'displayTitle': f'windows shell {SHA}'})
            self.artifact = True
            return ok()
        if args[:2] == ['run', 'download']:
            out = Path(args[args.index('-D') + 1])
            for name, data in self.files.items():
                (out / name).write_bytes(data)
            return ok()
        raise AssertionError(args)


def good_files():
    sums = f'{digest(EXE)}  HerdrShell.exe\n{digest(SETUP)}  HerdrShell-setup-{SHA}.exe\n'
    return {'HerdrShell.exe': EXE, f'HerdrShell-setup-{SHA}.exe': SETUP,
            'SHA256SUMS': sums.encode(), 'build-sha.txt': SHA.encode()}


def run(fake, fn):
    with mock.patch.object(subprocess, 'run', fake.run), mock.patch('time.sleep'), \
            mock.patch('time.monotonic', itertools.count(step=5).__next__), \
            contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        try:
            fn()
            return 0
        except SystemExit as stop:
            return stop.code


def fetch(dispatch=False):
    return lambda: pc.cmd_fetch(argparse.Namespace(sha=SHA, dispatch=dispatch))


def install(relaunch):
    return lambda: pc.cmd_install(argparse.Namespace(sha='', artifact=SHA, relaunch=relaunch))


def title(run_id, status, conclusion):
    return {'databaseId': run_id, 'status': status, 'conclusion': conclusion,
            'displayTitle': f'windows shell {SHA}'}


class FetchTests(unittest.TestCase):
    def test_only_a_whole_stamped_artifact_reaches_the_pc(self):
        files = good_files()
        no_stamp = {k: v for k, v in files.items() if k != 'build-sha.txt'}
        partial = dict(files, SHA256SUMS=f'{digest(EXE)}  HerdrShell.exe\n'.encode())
        corrupt = dict(files, **{'HerdrShell.exe': b'truncated'})
        for name, bad in (('no build stamp', no_stamp), ('partial sums', partial), ('corrupt exe', corrupt)):
            with self.subTest(name):
                fake = Fake(files=bad)
                self.assertNotEqual(run(fake, fetch()), 0)
                self.assertFalse([d for d in fake.scp if 'HerdrShell-' in d])

    def test_files_land_under_a_temp_name_and_move_after_a_pc_side_hash(self):
        fake = Fake()
        self.assertEqual(run(fake, fetch()), 0)
        shipped = [d for d in fake.scp if '/out/' in d]
        self.assertTrue(shipped and all(d.endswith('.part') for d in shipped), shipped)
        moves = [s for s in fake.remote if 'Move-Item' in s]
        self.assertEqual(len(moves), 2)
        self.assertTrue(all('Get-FileHash' in s for s in moves))
        self.assertIn(digest(EXE), moves[-1])
        # The checksum install_copy.ps1 trusts is written after both files are in place.
        self.assertIn(f'HerdrShell-{SHA}.sha256', fake.remote[-1])

    def test_a_pc_side_hash_mismatch_fails_the_fetch(self):
        fake = Fake(pc_hash_ok=False)
        self.assertNotEqual(run(fake, fetch()), 0)
        self.assertFalse(any('.sha256' in s for s in fake.remote))

    def test_a_failing_sha_is_not_dispatched_again_and_a_running_build_is_awaited(self):
        failed = [title(1, 'completed', 'failure'), title(2, 'completed', 'failure')]
        fake = Fake(runs=failed, artifact=False)
        self.assertEqual(run(fake, fetch(dispatch=True)), 4)
        self.assertEqual(fake.dispatched, 0)

        running = [dict(title(3, 'in_progress', None))]
        fake = Fake(runs=running, artifact=False)
        with mock.patch.object(pc, 'run_conclusion', return_value='failure'):
            self.assertEqual(run(fake, fetch(dispatch=True)), 4)
        self.assertEqual(fake.dispatched, 0)

        fake = Fake(runs=[title(1, 'completed', 'failure')], artifact=False)
        self.assertEqual(run(fake, fetch(dispatch=True)), 0)
        self.assertEqual(fake.dispatched, 1)


class InstallTests(unittest.TestCase):
    def modes(self, fake):
        return [a[2] if len(a) > 2 else 'install' for h, a in fake.helpers if h == 'install_copy.ps1']

    def test_a_wrong_running_commit_rolls_back(self):
        fake = Fake(commit='0' * 40)
        self.assertEqual(run(fake, install(relaunch=True)), 1)
        self.assertIn('-Rollback', self.modes(fake))
        self.assertNotIn('-MarkVerified', self.modes(fake))

    def test_the_right_commit_is_marked_verified(self):
        fake = Fake()
        self.assertEqual(run(fake, install(relaunch=True)), 0)
        self.assertEqual(self.modes(fake), ['-Relaunch', '-MarkVerified'])

    def test_without_relaunch_the_installed_exe_is_still_checked(self):
        fake = Fake()
        self.assertEqual(run(fake, install(relaunch=False)), 0)
        self.assertEqual(self.modes(fake), ['install', '-Check'])


if __name__ == '__main__':
    unittest.main()
