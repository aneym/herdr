"""Generated Windows process contracts, checked without touching the PC desktop.

Source inspection owns the launcher contract because PowerShell/task execution is
Windows-only. The Rust generator runs as a standalone binary; its output is the
cross-language handoff, including observable guard/backup/install/relaunch order.
"""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
UPDATE = SCRIPTS.parent / 'app/src-tauri/src/update.rs'


class LaunchUpdateTests(unittest.TestCase):
    def test_launch_task_rechecks_games_and_defaults_to_background(self):
        source = (SCRIPTS / 'launch.ps1').read_text()
        self.assertIn("$arg = if ($TestWindow) { '--test-window' } else { '--background' }", source)
        guard = source.index("Stop-IfGame 'HerdrShellLaunch'`r`n")
        start = source.index('Start-Process -FilePath')
        self.assertLess(guard, start)
        self.assertIn(". '$quotedGamecheck'`r`n", source)
        self.assertIn('-ArgumentList \'$arg\'', source)
        self.assertIn('-Value "$control$start"', source)

    def test_explicit_rollback_relaunch_does_not_require_a_live_app(self):
        # PowerShell handoff contract: a crashed build still gets its old build
        # relaunched on request. Start-App owns the separate game gate.
        source = (SCRIPTS / 'install_copy.ps1').read_text()
        rollback = next(line for line in source.splitlines() if line.startswith('if ($Rollback)'))
        self.assertIn('Undo "post-install check failed for $Sha" $Relaunch', rollback)
        self.assertNotIn('Get-App', rollback)

    def test_update_checks_games_before_starting_handoff_or_exiting(self):
        # The Windows subprocess boundary must reject the handoff while the
        # current app is still alive, not only in the detached updater.
        source = UPDATE.read_text()
        owner = source[source.index('#[cfg(windows)]\nfn install_inner'):source.index('#[cfg(test)]')]
        self.assertIn('if name != "previous.json" {', owner)
        self.assertIn('spawn_blocking(move || install(app, "staged.json"))', source)
        gate = owner.index("Stop-IfGame 'HerdrShellUpdate preflight'")
        refusal = owner.index('return Err(', gate)
        handoff = owner.index('let scheduled =')
        self.assertLess(gate, refusal)
        self.assertLess(refusal, handoff)
        self.assertLess(handoff, owner.index('app.exit(0)'))
        self.assertIn('include_str!("../../../scripts/gamecheck.ps1")', owner[:handoff])

    def test_standalone_rust_pipe_contracts(self):
        source = (UPDATE.parent / 'control.rs').read_text()
        # Only the Windows API module is excluded; the real retry policy and
        # its table-driven tests compile and execute unchanged.
        source = source[:source.index('#[cfg(windows)]')]
        with tempfile.TemporaryDirectory(prefix='shell-pipe-rust-') as tmp:
            path = Path(tmp) / 'control.rs'
            path.write_text(source)
            binary = Path(tmp) / 'control-tests'
            linker = ['-C', 'linker=/usr/bin/cc'] if sys.platform == 'darwin' else []
            subprocess.run(['rustc', '--edition=2021', *linker, '--test', str(path),
                            '-o', str(binary)], check=True)
            result = subprocess.run([str(binary), '--nocapture'], text=True, capture_output=True)
            print(result.stdout, end='')
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_standalone_rust_update_contracts(self):
        source = UPDATE.read_text()
        identity = source[source.index('fn same_sha('):source.index('// One source')]
        generator = source[source.index('fn update_script('):source.index('#[tauri::command]')]
        generator = generator.replace('../../../scripts/gamecheck.ps1', str(SCRIPTS / 'gamecheck.ps1'))
        tests = source[source.index('#[cfg(test)]'):]
        with tempfile.TemporaryDirectory(prefix='shell-update-rust-') as tmp:
            path = Path(tmp) / 'update.rs'
            path.write_text(identity + generator + tests)
            binary = Path(tmp) / 'update-tests'
            linker = ['-C', 'linker=/usr/bin/cc'] if sys.platform == 'darwin' else []
            subprocess.run(['rustc', '--edition=2021', *linker, '--test', str(path),
                            '-o', str(binary)], check=True)
            result = subprocess.run([str(binary), '--nocapture'], text=True, capture_output=True)
            print(result.stdout, end='')
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
