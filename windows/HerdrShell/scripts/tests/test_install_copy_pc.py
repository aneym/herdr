"""install_copy.ps1 on the real PC (Windows PowerShell 5.1) in a scratch directory.

Opt-in (HERDR_PC_TESTS=1) because it needs `ssh pc`; run it only while no game runs.
It never touches the installed Shell: the "app" is FakeShell.exe, a copy of a signed
system tool (ping.exe or waitfor.exe) started hidden in the ssh session by a stand-in
launch.ps1, and a stand-in gamecheck.ps1 reports a game from the Nth check on.

Regressions caught (Codex review of 3d26a4f0): a bad exe left installed or a good
.prev overwritten by an unverified build; a mismatched artifact swapped in; launching
while a game runs; relaunching without -Relaunch; a relaunch that asks for focus.
From the review of 7a6d36f1: a failed metadata write left in place; malformed metadata
trusted as verified; -Check passing an exe built from another commit (checked against
a real Shell build already on the PC, never launched); launch.ps1 starting its task
after a game began.
"""

import base64
import os
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pc_copy', SCRIPTS / 'pc.py')
pc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pc)

ROOT = 'C:/Users/aneym/winshell/install-copy-test'
SHA_A, SHA_B, SHA_C = 'a' * 40, 'b' * 40, 'c' * 40
# A real Shell build fetched earlier; its exe has this commit compiled in.
SHELL_SHA = '57faaaa8e0e7f31fb82717f77649dcf9186c9d0d'
SHELL_EXE = f'C:\\Users\\aneym\\winshell\\out\\HerdrShell-{SHELL_SHA}.exe'

GAMECHECK = r"""
# Stand-in: a game "runs" from check number game-at.txt on (0 = never).
function Get-Game {
    $n = 1 + [int](Get-Content (Join-Path $PSScriptRoot 'checks.txt') -ErrorAction SilentlyContinue)
    Set-Content (Join-Path $PSScriptRoot 'checks.txt') $n
    $at = [int](Get-Content (Join-Path $PSScriptRoot 'game-at.txt'))
    if ($at -and $n -ge $at) { return @('FakeGame.exe pid=1') }
    @()
}
function Stop-IfGame([string]$What) { if (@(Get-Game).Count) { exit 75 } }
"""

LAUNCH = r"""
param([string]$Exe, [switch]$TestWindow, [switch]$Background)
Add-Content (Join-Path $PSScriptRoot 'launches.txt') "background=$Background"
# Arguments that keep each stand-in tool alive for a minute; launch-args.txt 'die' breaks them.
$h = (Get-FileHash $Exe -Algorithm SHA256).Hash
$a = if ($h -eq (Get-FileHash 'C:\Windows\System32\PING.EXE' -Algorithm SHA256).Hash) { '-n 60 127.0.0.1' }
     else { '/T 60 HerdrFakeSignal' }
if ((Get-Content (Join-Path $PSScriptRoot 'launch-args.txt') -Raw).Trim() -eq 'die') { $a = '/nonexistent-flag' }
Start-Process -FilePath $Exe -ArgumentList $a -WindowStyle Hidden
"""

# Scenario setup: installed exe, optional .prev and metadata, the artifact to install.
SETUP = r"""
param([string]$Root, [string]$Installed, [string]$Prev, [string]$Meta, [string]$Artifact,
      [string]$Sha, [string]$Sum, [string]$GameAt, [string]$LaunchArgs, [switch]$Running,
      [switch]$MetaDir)
$ErrorActionPreference = 'Stop'
Get-Process FakeShell -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
$inst = Join-Path $Root 'inst'; $out = Join-Path $Root 'out'
foreach ($d in $inst, $out) { if (Test-Path $d) { Remove-Item $d -Recurse -Force }; New-Item -ItemType Directory $d | Out-Null }
$tools = @{ ping = 'C:\Windows\System32\PING.EXE'; waitfor = 'C:\Windows\System32\waitfor.exe'; where = 'C:\Windows\System32\where.exe'; shell = '__SHELL_EXE__' }
Copy-Item $tools[$Installed] (Join-Path $inst 'FakeShell.exe')
if ($Prev) { Copy-Item $tools[$Prev] (Join-Path $inst 'FakeShell.exe.prev') }
if ($Meta) { [IO.File]::WriteAllText((Join-Path $inst 'installed.json'), $Meta) }
# A directory where installed.json belongs makes the metadata write fail.
if ($MetaDir) { New-Item -ItemType Directory (Join-Path $inst 'installed.json') | Out-Null }
$src = Join-Path $out "HerdrShell-$Sha.exe"
Copy-Item $tools[$Artifact] $src
$hash = if ($Sum) { $Sum } else { (Get-FileHash $src -Algorithm SHA256).Hash }
Set-Content (Join-Path $out "HerdrShell-$Sha.sha256") $hash
Set-Content (Join-Path $Root 'scripts\checks.txt') 0
Set-Content (Join-Path $Root 'scripts\game-at.txt') $GameAt
Set-Content (Join-Path $Root 'scripts\launch-args.txt') $LaunchArgs
Remove-Item (Join-Path $Root 'scripts\launches.txt') -ErrorAction SilentlyContinue
if ($Running) { Start-Process (Join-Path $inst 'FakeShell.exe') -ArgumentList '-n 120 127.0.0.1' -WindowStyle Hidden; Start-Sleep 1 }
"""

# What the scenario left behind, one fact per line.
STATE = r"""
param([string]$Root)
$inst = Join-Path $Root 'inst'
$h = { param($p) if (Test-Path $p) { (Get-FileHash $p -Algorithm SHA256).Hash } else { 'none' } }
$tools = @{}
foreach ($t in 'PING.EXE', 'waitfor.exe', 'where.exe') { $tools[(Get-FileHash "C:\Windows\System32\$t" -Algorithm SHA256).Hash] = $t.Split('.')[0].ToLower() }
$tools[(Get-FileHash '__SHELL_EXE__' -Algorithm SHA256).Hash] = 'shell'
$name = { param($p) $x = & $h $p; if ($tools.ContainsKey($x)) { $tools[$x] } else { $x } }
"exe=$(& $name (Join-Path $inst 'FakeShell.exe'))"
"prev=$(& $name (Join-Path $inst 'FakeShell.exe.prev'))"
"new=$(& $name (Join-Path $inst 'FakeShell.exe.new'))"
"running=$(@(Get-Process FakeShell -ErrorAction SilentlyContinue).Count)"
$l = Join-Path $Root 'scripts\launches.txt'
"launches=$(if (Test-Path $l) { (Get-Content $l) -join ',' } else { '' })"
$m = Join-Path $inst 'installed.json'
"meta=$(if (Test-Path $m -PathType Leaf) { (Get-Content $m -Raw).Trim() } else { 'none' })"
Get-Process FakeShell -ErrorAction SilentlyContinue | Stop-Process -Force
"""

ALIVE, DIES = 'live', 'die'
MARK = '---state---'
LAUNCH_TASK = 'HerdrShellLaunchTest'


def ps(script, *args, timeout=120):
    cmd = f"{pc.PS} -File {ROOT}/scripts/{script} " + " ".join(args)
    p = subprocess.run(['ssh', 'pc', cmd], capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout.strip()


@unittest.skipUnless(os.environ.get('HERDR_PC_TESTS') == '1', 'set HERDR_PC_TESTS=1 to drive the PC')
class InstallCopyPCTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        rc, out = pc.remote(f"{pc.PS} -File C:/Users/aneym/winshell/scripts/game_guard.ps1")
        if rc != 0:
            raise unittest.SkipTest(f'a game runs on the PC: {out}')
        pc.remote(f"{pc.PS} -Command \"New-Item -ItemType Directory -Force -Path '{ROOT}/scripts' | Out-Null\"")
        with tempfile.TemporaryDirectory() as d:
            # The real launch.ps1 under a task name of its own, beside the stand-in gamecheck.
            real_launch = (SCRIPTS / 'launch.ps1').read_text().replace('HerdrShellLaunch', LAUNCH_TASK)
            files = {'gamecheck.ps1': GAMECHECK, 'launch.ps1': LAUNCH, 'setup.ps1': SETUP,
                     'state.ps1': STATE, 'real-launch.ps1': real_launch}
            for name, text in files.items():
                (Path(d) / name).write_text(text.replace('__SHELL_EXE__', SHELL_EXE))
                assert pc.scp_to(Path(d) / name, f'{ROOT}/scripts/{name}') == 0
        assert pc.scp_to(SCRIPTS / 'install_copy.ps1', f'{ROOT}/scripts/install_copy.ps1') == 0

    def scenario(self, *, installed='ping', prev='', meta='', artifact='waitfor', sha=SHA_B, sum_='',
                 game_at=0, launch=ALIVE, running=False, flags=('-Relaunch',), meta_dir=False):
        args = [f'-Root {ROOT}', f'-Installed {installed}', f'-Artifact {artifact}', f'-Sha {sha}',
                f'-GameAt {game_at}', f"-LaunchArgs '{launch}'"]
        if prev:
            args.append(f'-Prev {prev}')
        if meta:
            args.append(f"-Meta '{meta}'")
        if sum_:
            args.append(f'-Sum {sum_}')
        if running:
            args.append('-Running')
        if meta_dir:
            args.append('-MetaDir')
        # One ssh session for all three: Windows OpenSSH ends a session's processes when
        # it closes, so a "running" app started by setup.ps1 must share it with the install.
        install = ' '.join([f'-Sha {sha}', *flags, f'-InstallDir {ROOT}/inst', f'-OutDir {ROOT}/out',
                            '-ExeName FakeShell.exe', '-UpSeconds 2'])
        script = (f"& '{ROOT}/scripts/setup.ps1' {' '.join(args)}\n"
                  # An uncaught error leaves this 1, as powershell -File would exit.
                  f"$global:LASTEXITCODE = 1\n& '{ROOT}/scripts/install_copy.ps1' {install}\n"
                  f"$rc = $LASTEXITCODE\n'{MARK}'\n"
                  f"& '{ROOT}/scripts/state.ps1' -Root {ROOT}\nexit $rc\n")
        enc = base64.b64encode(script.encode('utf-16-le')).decode()
        p = subprocess.run(['ssh', 'pc', f'{pc.PS} -EncodedCommand {enc}'], capture_output=True,
                           text=True, timeout=180)
        out, _, facts = p.stdout.partition(MARK)
        self.assertTrue(facts, p.stdout + p.stderr)
        state = dict(line.split('=', 1) for line in facts.splitlines() if '=' in line)
        return p.returncode, out.strip(), state

    def test_a_good_build_swaps_in_without_focus_and_keeps_the_old_one(self):
        rc, out, s = self.scenario()
        self.assertEqual(rc, 0, out)
        self.assertEqual((s['exe'], s['prev'], s['new']), ('waitfor', 'ping', 'none'))
        self.assertEqual(s['launches'], 'background=True')
        self.assertIn('"verified":false', s['meta'])

    def test_a_build_that_dies_is_rolled_back(self):
        rc, out, s = self.scenario(artifact='where', launch=DIES)
        self.assertEqual(rc, 1, out)
        self.assertEqual((s['exe'], s['prev']), ('ping', 'ping'))

    def test_an_unverified_install_never_replaces_a_good_prev(self):
        meta = f'{{"sha":"{SHA_B}","sha256":"x","verified":false}}'
        rc, out, s = self.scenario(installed='waitfor', prev='ping', meta=meta, artifact='ping', sha=SHA_C)
        self.assertEqual(rc, 0, out)
        # A replaced .prev would now hold the unverified waitfor build.
        self.assertEqual((s['exe'], s['prev']), ('ping', 'ping'))

    def test_a_mismatched_artifact_changes_nothing(self):
        rc, out, s = self.scenario(sum_='0' * 64)
        self.assertEqual(rc, 1, out)
        self.assertEqual((s['exe'], s['new'], s['launches']), ('ping', 'none', ''))

    def test_a_game_before_the_swap_leaves_the_build_staged(self):
        rc, out, s = self.scenario(game_at=1)
        self.assertEqual(rc, 76, out)
        self.assertEqual((s['exe'], s['new'], s['launches']), ('ping', 'waitfor', ''))

    def test_a_game_before_the_launch_starts_nothing(self):
        rc, out, s = self.scenario(game_at=2)
        self.assertEqual(rc, 76, out)
        self.assertEqual((s['exe'], s['launches'], s['running']), ('waitfor', '', '0'))

    def test_a_running_app_is_left_alone_without_relaunch(self):
        rc, out, s = self.scenario(running=True, flags=())
        self.assertEqual(rc, 77, out)
        self.assertEqual((s['exe'], s['new'], s['running'], s['launches']), ('ping', 'waitfor', '1', ''))

    def test_a_failed_metadata_write_rolls_back(self):
        rc, out, s = self.scenario(flags=(), meta_dir=True)
        self.assertEqual(rc, 1, out)
        self.assertEqual(s['exe'], 'ping')

    def test_malformed_metadata_is_unverified(self):
        rc, out, s = self.scenario(installed='waitfor', prev='ping', meta='not json',
                                   artifact='where', flags=())
        self.assertEqual(rc, 0, out)
        self.assertEqual((s['exe'], s['prev']), ('where', 'ping'))

    def test_check_reads_the_commit_compiled_into_the_exe(self):
        # installed.json and the checksum both claim SHA_B; the exe was built from SHELL_SHA.
        meta = f'{{"sha":"{SHA_B}","sha256":"x","verified":false}}'
        rc, out, s = self.scenario(installed='shell', prev='ping', meta=meta, artifact='shell',
                                   sha=SHA_B, flags=('-Check',))
        self.assertEqual(rc, 1, out)
        self.assertEqual(s['exe'], 'ping')
        rc, out, s = self.scenario(installed='shell', prev='ping', artifact='shell',
                                   sha=SHELL_SHA, flags=('-Check',))
        self.assertEqual(rc, 0, out)
        self.assertEqual(s['exe'], 'shell')

    def test_launch_rechecks_for_a_game_before_it_starts_the_task(self):
        rc, out = ps('setup.ps1', f'-Root {ROOT}', '-Installed ping', '-Artifact ping', f'-Sha {SHA_A}',
                     '-GameAt 1', "-LaunchArgs 'live'")
        self.assertEqual(rc, 0, out)
        try:
            rc, out = ps('real-launch.ps1', '-Exe C:/Windows/System32/rundll32.exe')
            _rc, result = pc.remote(f"{pc.PS} -Command \"(Get-ScheduledTaskInfo {LAUNCH_TASK}).LastTaskResult\"")
        finally:
            pc.remote(f"{pc.PS} -Command \"Unregister-ScheduledTask {LAUNCH_TASK} -Confirm:$false\"")
        self.assertEqual(rc, 75, out)
        # 267011 (0x41303): the task has not yet run.
        self.assertEqual(result.strip().splitlines()[-1], '267011')


if __name__ == '__main__':
    unittest.main()
