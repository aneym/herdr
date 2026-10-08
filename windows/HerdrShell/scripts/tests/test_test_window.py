"""CLI control routing at the ssh/scp edge, without a PC.

Real argument parsing and gated helper encoding must preserve -Test for shots
and all control commands (including drags); production must remain unchanged.
"""

import base64
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shlex
import subprocess
import sys
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pc_test_window', SCRIPTS / 'pc.py')
pc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pc)
REAL_RUN = subprocess.run


class TestWindowRoutingTests(unittest.TestCase):
    def command(self, arguments):
        sent = []

        def run(argv, *args, **kwargs):
            if argv[0] in ('ssh', 'scp'):
                sent.append(argv)
                return subprocess.CompletedProcess(argv, 0, '{"ok":true}\n', '')
            return REAL_RUN(argv, *args, **kwargs)

        with mock.patch.object(sys, 'argv', ['pc.py', *arguments]), \
                mock.patch.object(subprocess, 'run', run), \
                contextlib.redirect_stdout(io.StringIO()):
            try:
                pc.main()
            except SystemExit as stop:
                self.assertEqual(stop.code, 0)
        controls = []
        for argv in sent:
            if argv[0] == 'ssh':
                parts = shlex.split(argv[2])
                if '-Script' in parts and parts[parts.index('-Script') + 1] == 'ctl.ps1':
                    encoded = parts[parts.index('-ArgsB64') + 1]
                    controls.append(json.loads(base64.b64decode(encoded)))
        self.assertEqual(len(controls), 1)
        return controls[0]

    def test_control_and_shot_route_to_selected_instance(self):
        for test_window in (False, True):
            for command in ('ui', 'drag_pin', 'drag_divider', 'shot'):
                with self.subTest(test_window=test_window, command=command):
                    args = (['shot', '--out', 'captured.png'] if command == 'shot'
                            else ['ctl', json.dumps({'cmd': command})])
                    if test_window:
                        args.append('--test-window')
                    helper_args = self.command(args)
                    self.assertEqual('-Test' in helper_args, test_window)
                    payload = json.loads(base64.b64decode(helper_args[helper_args.index('-JsonB64') + 1]))
                    self.assertEqual(payload['cmd'], command)


if __name__ == '__main__':
    unittest.main()
