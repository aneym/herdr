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


    def test_screenshot_hooks_refuse_production_before_any_remote_call(self):
        for command in ('open_detail', 'row_menu', 'paste_image'):
            with self.subTest(command=command), \
                    mock.patch.object(sys, 'argv', ['pc.py', 'ctl', json.dumps({'cmd': command})]), \
                    mock.patch.object(subprocess, 'run') as remote, \
                    contextlib.redirect_stderr(io.StringIO()) as error:
                with self.assertRaises(SystemExit) as stop:
                    pc.main()
                self.assertEqual(stop.exception.code, 2)
                self.assertIn('--test-window', error.getvalue())
                remote.assert_not_called()
            helper_args = self.command(['ctl', json.dumps({'cmd': command}), '--test-window'])
            self.assertIn('-Test', helper_args)

    def test_acceptance_scripts_route_their_first_control_command(self):
        class Captured(Exception):
            pass

        for name, expected in [('check_pin_drag', 'ping'), ('check_pane_drag', 'ui'),
                               ('theme_check', 'appearance')]:
            for test_window in (False, True):
                with self.subTest(script=name, test_window=test_window):
                    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f'{name}.py')
                    module = importlib.util.module_from_spec(spec)
                    spec.loader.exec_module(module)
                    sent = []

                    def run(argv, *args, **kwargs):
                        if argv[0] in ('ssh', 'scp'):
                            if argv[0] == 'ssh':
                                parts = shlex.split(argv[2])
                                if '-Script' in parts and parts[parts.index('-Script') + 1] == 'ctl.ps1':
                                    sent.append(json.loads(base64.b64decode(parts[parts.index('-ArgsB64') + 1])))
                                    raise Captured()
                            return subprocess.CompletedProcess(argv, 0, '{"game":false}\n', '')
                        return REAL_RUN(argv, *args, **kwargs)

                    arguments = [f'{name}.py']
                    if name == 'theme_check':
                        arguments += ['--out-dir', str(SCRIPTS)]
                    if test_window:
                        arguments.append('--test-window')
                    with mock.patch.object(sys, 'argv', arguments), \
                            mock.patch.object(subprocess, 'run', run), \
                            contextlib.redirect_stdout(io.StringIO()):
                        with self.assertRaises(Captured):
                            module.main(arguments[1:])
                    self.assertTrue(sent)
                    for helper_args in sent:
                        self.assertEqual('-Test' in helper_args, test_window)
                        payload = json.loads(base64.b64decode(helper_args[helper_args.index('-JsonB64') + 1]))
                        self.assertEqual(payload['cmd'], expected)

    def test_pane_drag_screenshot_passes_instance_to_pc_cli(self):
        spec = importlib.util.spec_from_file_location('pane_shot', SCRIPTS / 'check_pane_drag.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            module.EVIDENCE = Path(directory)
            for test_window in (False, True):
                module.TEST_WINDOW = test_window
                sent = []

                def run(argv, *args, **kwargs):
                    sent.append(argv)
                    return subprocess.CompletedProcess(argv, 0, '', '')

                with mock.patch.object(subprocess, 'run', run):
                    module.shot('routing')
                self.assertEqual(len(sent), 1)
                self.assertEqual('--test-window' in sent[0], test_window)


if __name__ == '__main__':
    unittest.main()
