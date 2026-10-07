"""Every pc.py command that can touch the app reaches the PC only through gated.ps1.

Integration at the ssh edge: pc.py's real subcommands run against a fake PC that
records each remote command. A regression that calls install.ps1, launch.ps1 or
ctl.ps1 directly (the 2026-10-06 breach, where a separate guard result did not
stop an update apply) fails here. The live check-and-act happens in gated.ps1 on
the PC; this owns the Studio side's routing and its stop on exit 75.
"""

import argparse
import importlib.util
from pathlib import Path
import re
import subprocess
import sys
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pc', SCRIPTS / 'pc.py')
pc = importlib.util.module_from_spec(spec)
sys.modules['pc'] = pc
spec.loader.exec_module(pc)

PROBES = {'game_guard.ps1', 'status.ps1', 'idle_refresh.ps1'}


class FakePC:
    """Stands in for ssh/scp; `gated_rc` is what gated.ps1 answers."""

    def __init__(self, gated_rc):
        self.gated_rc = gated_rc
        self.helpers = []  # (file run by -File, gated helper or None)

    def run(self, argv, **_kwargs):
        if argv[0] == 'scp':
            return subprocess.CompletedProcess(argv, 0)
        cmd = argv[2]
        found = re.search(r'-File \S+/(\S+\.ps1)', cmd)
        if not found:
            return subprocess.CompletedProcess(argv, 0, '', '')
        script = found.group(1)
        inner = re.search(r'-Script (\S+)', cmd)
        self.helpers.append((script, inner.group(1) if inner else None))
        out = {'game_guard.ps1': '{"game":false,"procs":[],"idle_s":999}',
               'idle_refresh.ps1': '{"idle_s":999}',
               'status.ps1': '{"exe":"C:\\\\Herdr Shell\\\\HerdrShell.exe"}'}.get(
                   script, '{"ok":true,"machine":{"state":"up"},"rows":[],"panes":[]}')
        rc = self.gated_rc if script == 'gated.ps1' else 0
        return subprocess.CompletedProcess(argv, rc, out if rc == 0 else 'GATED', '')


def run_command(fake, fn, args):
    with mock.patch.object(pc.subprocess, 'run', fake.run), \
         mock.patch('builtins.print'):
        try:
            result = fn(args)
        except SystemExit as stop:
            return stop.code
        return result


COMMANDS = [
    ('install', pc.cmd_install, argparse.Namespace(sha='a' * 40, relaunch=True), 'install.ps1'),
    ('ctl', pc.cmd_ctl, argparse.Namespace(json='{"cmd":"update","action":"apply"}'), 'ctl.ps1'),
    ('run', pc.cmd_run, argparse.Namespace(force_idle=True, test_window=False), 'launch.ps1'),
    ('shot', pc.cmd_shot, argparse.Namespace(out='/nonexistent/shot.png'), 'ctl.ps1'),
]


class GateTests(unittest.TestCase):
    def test_app_helpers_only_run_through_the_gate(self):
        for name, fn, args, helper in COMMANDS:
            with self.subTest(name):
                fake = FakePC(gated_rc=0)
                run_command(fake, fn, args)
                direct = [s for s, _ in fake.helpers if s not in PROBES | {'gated.ps1'}]
                self.assertEqual(direct, [])
                self.assertIn(helper, [inner for s, inner in fake.helpers if s == 'gated.ps1'])

    def test_a_running_game_stops_the_command_at_its_first_action(self):
        for name, fn, args, _helper in COMMANDS:
            with self.subTest(name):
                fake = FakePC(gated_rc=75)
                self.assertEqual(run_command(fake, fn, args), 75)
                self.assertEqual([s for s, _ in fake.helpers if s == 'gated.ps1'], ['gated.ps1'])


if __name__ == '__main__':
    unittest.main()
