"""Resolve optional Studio build sharing for the just entry point."""

import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys


def studio_build():
    root = Path(__file__).resolve().parent.parent
    return platform.system() == "Darwin" and root.is_relative_to(
        Path("/Volumes/StudioExt/repos")
    ) and not os.environ.get("CI")


def target_dir():
    if os.environ.get("CARGO_TARGET_DIR"):
        return os.environ["CARGO_TARGET_DIR"]
    if studio_build():
        shared = Path("/Volumes/StudioExt/repos/herdr-target")
        try:
            shared.mkdir(exist_ok=True)
            if os.access(shared, os.W_OK):
                return str(shared)
        except OSError as exc:
            print(f"shared cargo target unavailable: {exc}; using target/", file=sys.stderr)
        else:
            print("shared cargo target not writable; using target/", file=sys.stderr)
    return "target"


def rustc_wrapper():
    if os.environ.get("RUSTC_WRAPPER"):
        return os.environ["RUSTC_WRAPPER"]
    if not studio_build() or target_dir() == "target":
        return ""
    cache = shutil.which("sccache")
    if cache:
        try:
            subprocess.run(
                [cache, "--version"], check=True, timeout=5,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            return cache
        except (OSError, subprocess.SubprocessError):
            pass
    print("sccache unavailable; using rustc directly", file=sys.stderr)
    return ""


if __name__ == "__main__":
    if sys.argv[1:] == ["target"]:
        print(target_dir())
    elif sys.argv[1:] == ["wrapper"]:
        print(rustc_wrapper())
    else:
        sys.exit("usage: cargo_shared.py target|wrapper")
