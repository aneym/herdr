"""Every Studio-side command that can touch the PC app goes through gated.ps1 and stops at its 75.

Integration at the ssh edge: the real pc.py, theme_check.py and check_pin_drag.py run
against a fake PC that records each remote command and answers as the PC would. A
regression that calls a helper directly (the 2026-10-06 breach, where a separate guard
result did not stop an update apply), or that keeps acting after the gate refused once,
fails here. gated.ps1 itself runs on the PC; tests/test_gate_pc.py drives it there.
"""

import argparse
import base64
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
modules = {}
for name in ('pc', 'theme_check', 'check_pin_drag'):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f'{name}.py')
    modules[name] = importlib.util.module_from_spec(spec)
    sys.modules[name] = modules[name]
    spec.loader.exec_module(modules[name])
pc, theme_check, pin_drag = modules['pc'], modules['theme_check'], modules['check_pin_drag']

PROBES = {'game_guard.ps1', 'status.ps1', 'idle_refresh.ps1'}


def png(width, height, grey):
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    raw = b''.join(b'\0' + bytes([grey]) * 3 * width for _ in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))


WINDOW_SHOT = png(40, 40, 128)


class FakePC:
    """Stands in for ssh, scp and the Studio herdr CLI. The gated call numbered
    `refuse_at` (from 0) answers 75, as gated.ps1 does when a game is up."""

    def __init__(self, refuse_at=None):
        self.refuse_at = refuse_at
        self.gated = []   # helpers run through gated.ps1, in order
        self.direct = []  # helpers run without it
        self.herdr = []

    def run(self, argv, **_kwargs):
        if argv[0] == 'scp':
            if not argv[-1].startswith('pc:'):
                Path(argv[-1]).write_bytes(WINDOW_SHOT)
            return subprocess.CompletedProcess(argv, 0)
        if argv[0] == 'herdr':
            self.herdr.append(argv[1:3])
            return subprocess.CompletedProcess(argv, 0, json.dumps({'result': herdr_reply(argv[1:])}), '')
        script = re.search(r'-File \S+/(\S+\.ps1)', argv[2])
        if not script:
            return subprocess.CompletedProcess(argv, 0, '', '')
        script = script.group(1)
        if script != 'gated.ps1':
            self.direct.append(script)
            out = {'game_guard.ps1': '{"game":false,"procs":[],"idle_s":999}',
                   'idle_refresh.ps1': '{"idle_s":999}',
                   'status.ps1': '{"exe":"C:\\\\Herdr Shell\\\\HerdrShell.exe"}'}.get(script, '')
            return subprocess.CompletedProcess(argv, 0, out, '')
        helper = re.search(r'-Script (\S+)', argv[2]).group(1)
        args = json.loads(base64.b64decode(re.search(r'-ArgsB64 (\S+)', argv[2]).group(1)))
        self.gated.append(helper)
        if len(self.gated) - 1 == self.refuse_at:
            return subprocess.CompletedProcess(argv, 75, 'GATED: game running', '')
        return subprocess.CompletedProcess(argv, 0, helper_reply(helper, args), '')


def helper_reply(helper, args):
    if helper != 'ctl.ps1':
        return json.dumps({'installer': 'x', 'exitcode': 0, 'exe': 'x'})
    cmd = json.loads(base64.b64decode(args[args.index('-JsonB64') + 1]))
    if cmd['cmd'] == 'appearance':
        return json.dumps({'ok': True, 'mode': cmd['mode']})
    return json.dumps({'ok': True, 'commit': 'c', 'machine': {'state': 'up'}, 'rows': [], 'panes': [],
                       'appearance': {'mode': 'light', 'override': 'system'}})


def herdr_reply(args):
    if args[:2] == ['workspace', 'create']:
        return {'workspace': {'workspace_id': 'w'}, 'tab': {'tab_id': 't0'}}
    if args[:2] == ['tab', 'create']:
        return {'tab': {'tab_id': args[-2]}, 'root_pane': {'pane_id': 'p'}}
    if args[:2] == ['tab', 'list']:
        return {'tabs': [{'tab_id': t, 'pin_index': i, 'role': 'agent'} for i, t in enumerate(('t0', 'drag-b', 'drag-c'))]}
    return {}


def outcome(fake, fn):
    """Exit status of one command run against the fake PC."""
    with mock.patch.object(subprocess, 'run', fake.run), \
         mock.patch.object(pin_drag, 'OUT', Path(tempfile.mkdtemp()) / 'PIN-DRAG.txt'), \
         mock.patch.object(sys, 'argv', ['theme_check', '--out-dir', tempfile.mkdtemp()]), \
         mock.patch('time.sleep'), \
         contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        try:
            return fn() or 0
        except SystemExit as stop:
            return stop.code


def ns(**kw):
    return argparse.Namespace(**kw)


# name: (entry point, exit status with no game; None when the fake cannot satisfy its checks)
COMMANDS = {
    'install --relaunch': (lambda: pc.cmd_install(ns(sha='a' * 40, relaunch=True)), 0),
    'ctl update apply': (lambda: pc.cmd_ctl(ns(json='{"cmd":"update","action":"apply"}')), 0),
    'run': (lambda: pc.cmd_run(ns(force_idle=True, test_window=False)), 0),
    'shot': (lambda: pc.cmd_shot(ns(out=str(Path(tempfile.mkdtemp()) / 'shot.png'))), 0),
    'stage': (lambda: pc.ps_file('stage.ps1', '-Sha', 'a' * 40)[0], 0),
    'theme_check': (theme_check.main, 1),  # a mid-grey shot is neither light nor dark
}


class GateTests(unittest.TestCase):
    def test_commands_reach_the_app_only_through_the_gate(self):
        for name, (fn, expected) in COMMANDS.items():
            with self.subTest(name):
                fake = FakePC()
                rc = outcome(fake, fn)
                self.assertEqual(set(fake.direct) - PROBES, set())
                self.assertTrue(fake.gated)
                if expected is not None:
                    self.assertEqual(rc, expected)

    def test_a_refusal_at_any_step_ends_the_command_with_75(self):
        for name, (fn, _expected) in COMMANDS.items():
            nominal = FakePC()
            outcome(nominal, fn)
            for step in range(len(nominal.gated)):
                with self.subTest(name, step=step):
                    fake = FakePC(refuse_at=step)
                    self.assertEqual(outcome(fake, fn), 75)
                    self.assertEqual(len(fake.gated), step + 1)

    def test_pin_drag_closes_its_server_workspace_and_stops_at_a_refusal(self):
        fake = FakePC(refuse_at=2)  # ping, ui, then the first wait on the throwaway pins
        self.assertEqual(outcome(fake, pin_drag.main), 75)
        self.assertEqual(len(fake.gated), 3)
        self.assertIn(['workspace', 'close'], fake.herdr)


if __name__ == '__main__':
    unittest.main()
