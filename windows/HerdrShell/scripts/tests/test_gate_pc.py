"""gated.ps1 on the real PC (Windows PowerShell 5.1): arguments and exit codes reach the helper.

Opt-in (HERDR_PC_TESTS=1) because it needs `ssh pc`. It uploads gated.ps1, gamecheck.ps1
and a sentinel helper to a scratch directory and never touches the app. A game running
on the PC skips the argument cases after asserting the refusal: 75 and no sentinel output.
"""

import base64
import json
import os
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pc', SCRIPTS / 'pc.py')
pc = importlib.util.module_from_spec(spec)
sys.modules['pc'] = pc
spec.loader.exec_module(pc)

REMOTE = f'{pc.W}/gate-test'
SENTINEL = """param([string]$Exe, [switch]$TestWindow)
Write-Output ("SENTINEL exe=[{0}] window={1}" -f $Exe, $TestWindow)
exit 7
"""


@unittest.skipUnless(os.environ.get('HERDR_PC_TESTS') == '1', 'set HERDR_PC_TESTS=1 to drive the PC')
class GatePCTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        pc.remote(f"{pc.PS} -Command \"New-Item -ItemType Directory -Force -Path '{REMOTE}' | Out-Null\"")
        with tempfile.TemporaryDirectory() as directory:
            sentinel = Path(directory) / 'sentinel.ps1'
            sentinel.write_text(SENTINEL)
            for path in (SCRIPTS / 'gated.ps1', SCRIPTS / 'gamecheck.ps1', sentinel):
                assert pc.scp_to(path, f'{REMOTE}/{path.name}') == 0

    def gated(self, *args):
        b64 = base64.b64encode(json.dumps(list(args)).encode()).decode()
        p = subprocess.run(['ssh', 'pc', f'{pc.PS} -File {REMOTE}/gated.ps1 -Script sentinel.ps1 -ArgsB64 {b64}'],
                           capture_output=True, text=True, timeout=60)
        return p.returncode, p.stdout.strip()

    def test_arguments_and_exit_code_reach_the_helper(self):
        rc, out = self.gated()
        if rc == 75:
            self.assertNotIn('SENTINEL', out)
            self.skipTest(f'a game runs on the PC: {out}')
        cases = [
            ((), 'exe=[] window=False'),
            (('-TestWindow',), 'exe=[] window=True'),
            (('-Exe', 'C:\\Users\\a b\\Herdr Shell\\HerdrShell.exe', '-TestWindow'),
             'exe=[C:\\Users\\a b\\Herdr Shell\\HerdrShell.exe] window=True'),
        ]
        for args, expected in cases:
            with self.subTest(args=args):
                rc, out = self.gated(*args)
                self.assertEqual((rc, out), (7, f'SENTINEL {expected}'))


if __name__ == '__main__':
    unittest.main()
