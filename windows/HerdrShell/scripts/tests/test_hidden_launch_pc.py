"""Real hidden launcher/task regression, opt-in and isolated from the installed Shell.

HERDR_PC_TESTS=1 requires ssh pc and no game. The scheduled wscript/PowerShell
handoff starts only waitfor.exe with a unique signal; cleanup targets that signal
and HerdrShellLaunchTest, never the app or its normal task. The source guard is
explicitly requested as cheap local evidence alongside the Windows integration.
"""

import base64
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import uuid

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pc_hidden_launch', SCRIPTS / 'pc.py')
pc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pc)

VBS_VALUE = '''('WScript.Quit CreateObject("WScript.Shell").Run("' + $cmd.Replace('"', '""') + '", 0, True)')'''


class HiddenLaunchSourceTests(unittest.TestCase):
    def test_vbs_quotes_are_escaped_outside_expandable_strings(self):
        for name in ('launch.ps1', 'idle_refresh.ps1'):
            with self.subTest(script=name):
                source = (SCRIPTS / name).read_text()
                line = next(line for line in source.splitlines()
                            if line.startswith('Set-Content -LiteralPath $vbs'))
                self.assertIn('-Value ' + VBS_VALUE, line)


@unittest.skipUnless(os.environ.get('HERDR_PC_TESTS') == '1',
                     'set HERDR_PC_TESTS=1 to drive the PC')
class HiddenLaunchPCTests(unittest.TestCase):
    def test_real_wscript_task_starts_standin_and_reports_success(self):
        rc, out = pc.remote(f'{pc.PS} -File {pc.R_SCRIPTS}/game_guard.ps1', timeout=60)
        if rc == 75:
            self.skipTest(f'a game runs on the PC: {out}')
        self.assertEqual(rc, 0, out)
        token = uuid.uuid4().hex
        root = f'{pc.W}/hidden launch test {token}'
        signal = f'HerdrLaunchTest{token}'
        rc, out = pc.remote(f'''{pc.PS} -Command "New-Item -ItemType Directory -Path '{root}' -ErrorAction Stop | Out-Null"''', timeout=60)
        self.assertEqual(rc, 0, out)
        # The output directory contains spaces to exercise the nested -File quotes.
        driver = r"""
$ErrorActionPreference = 'Stop'
$task = 'HerdrShellLaunchTest'
$root = '__ROOT__'
$signal = '__SIGNAL__'
if (Get-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue) {
    throw 'Test task already exists; refusing to overwrite it'
}
try {
    & "$root/launch.ps1" -Exe 'C:\Windows\System32\waitfor.exe' -TaskName $task -ScriptDir $root -TestArguments "/T 30 $signal"
    $deadline = (Get-Date).AddSeconds(20)
    $started = $false
    do {
        $processes = @(Get-CimInstance Win32_Process -Filter "Name = 'waitfor.exe'" | Where-Object { $_.CommandLine -like "*$signal*" })
        if ($processes.Count -gt 0) { $started = $true }
        $info = Get-ScheduledTaskInfo -TaskName $task
        $state = (Get-ScheduledTask -TaskName $task).State
        if ($started -and $state -ne 'Running' -and $info.LastTaskResult -eq 0) { break }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $deadline)
    "started=$started"
    "result=$($info.LastTaskResult)"
    "state=$state"
    if (-not $started -or $state -eq 'Running' -or $info.LastTaskResult -ne 0) {
        throw 'Hidden task did not start waitfor and complete successfully'
    }
} finally {
    Stop-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $task -Confirm:$false -ErrorAction SilentlyContinue
    Get-CimInstance Win32_Process -Filter "Name = 'waitfor.exe'" |
        Where-Object { $_.CommandLine -like "*$signal*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
""".replace('__ROOT__', root).replace('__SIGNAL__', signal)
        try:
            for name in ('launch.ps1', 'gamecheck.ps1'):
                self.assertEqual(pc.scp_to(SCRIPTS / name, f'{root}/{name}'), 0)
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'driver.ps1'
                path.write_text(driver)
                self.assertEqual(pc.scp_to(path, f'{root}/driver.ps1'), 0)
            # Encoded command avoids SSH/PowerShell path quoting differences.
            command = f"& '{root}/driver.ps1'"
            encoded = base64.b64encode(command.encode('utf-16-le')).decode()
            p = subprocess.run(['ssh', 'pc', f'{pc.PS} -EncodedCommand {encoded}'],
                               capture_output=True, text=True, timeout=90)
            self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
            self.assertIn('started=True', p.stdout)
            self.assertIn('result=0', p.stdout)
        finally:
            rc, out = pc.remote(f'''{pc.PS} -Command "Remove-Item -LiteralPath '{root}' -Recurse -Force -ErrorAction Stop"''', timeout=60)
            self.assertEqual(rc, 0, out)


if __name__ == '__main__':
    unittest.main()
