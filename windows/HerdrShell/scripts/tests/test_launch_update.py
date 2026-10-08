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
