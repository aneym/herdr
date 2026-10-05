"""Integration contract for just's build environment, without compiling Rust.

Protects artifact placement and caller overrides at the subprocess boundary.
Dropping the exports or ignoring CI/overrides fails here; existing build tests
cover compilation, not worktree placement. No test-only production seam.
"""

import os
from pathlib import Path
import platform
import subprocess
import shutil
import unittest


class CargoSharedTests(unittest.TestCase):
    def test_just_environment(self):
        root = Path(__file__).resolve().parent.parent
        studio = platform.system() == "Darwin" and root.is_relative_to(
            Path("/Volumes/StudioExt/repos")
        )
        for extra, target, wrapper in [
            ({}, "/Volumes/StudioExt/repos/herdr-target" if studio else "target",
             shutil.which("sccache") or "" if studio else ""),
            ({"CI": "true"}, "target", ""),
            ({"CARGO_TARGET_DIR": "/caller/target", "RUSTC_WRAPPER": "/caller/cache"},
             "/caller/target", "/caller/cache"),
        ]:
            with self.subTest(extra=extra):
                env = {k: v for k, v in os.environ.items()
                       if k not in {"CI", "CARGO_TARGET_DIR", "RUSTC_WRAPPER"}}
                env.update(extra)
                for key, expected in [("CARGO_TARGET_DIR", target), ("RUSTC_WRAPPER", wrapper)]:
                    result = subprocess.run(
                        ["just", "--evaluate", key], cwd=root, env=env,
                        capture_output=True, text=True, check=True, timeout=15,
                    ).stdout.strip()
                    if expected is not None:
                        self.assertEqual(result, expected)
                    elif result:
                        self.assertTrue(Path(result).is_file())


if __name__ == "__main__":
    unittest.main()
