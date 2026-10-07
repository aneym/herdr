"""Compile the Tauri shell, preferring the installed Windows MSVC target."""

from pathlib import Path
import os
import shutil
import struct
import subprocess
import zlib


ROOT = Path(__file__).resolve().parent.parent
SHELL = ROOT / "windows/HerdrShell/app/src-tauri"
WINDOWS_TARGET = "x86_64-pc-windows-msvc"


def write_missing(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    # Never replace a real icon, including one created concurrently.
    try:
        with path.open("xb") as output:
            output.write(data)
    except FileExistsError:
        pass


def ensure_icons():
    icon = SHELL / "icons/icon.ico"
    # One 16x16, 32-bit DIB image with an opaque BGRA bitmap and empty AND mask.
    pixels = bytes((0x80, 0x60, 0x40, 0xFF)) * (16 * 16)
    mask = bytes(4 * 16)
    dib = struct.pack("<IiiHHIIiiII", 40, 16, 32, 1, 32, 0, len(pixels), 0, 0, 0, 0)
    image = dib + pixels + mask
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", 16, 16, 0, 0, 1, 32, len(image), 22)
    write_missing(icon, header + entry + image)

    # generate_context! also reads the first PNG icon on the host target.
    def chunk(kind, data):
        return (
            struct.pack(">I", len(data)) + kind + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", 32, 32, 8, 6, 0, 0, 0))
    rows = (b"\0" + bytes((0x40, 0x60, 0x80, 0xFF)) * 32) * 32
    png += chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    write_missing(SHELL / "icons/32x32.png", png)


def main():
    installed = subprocess.check_output(
        ["rustup", "target", "list", "--installed"], text=True
    ).splitlines()
    env = os.environ.copy()
    if WINDOWS_TARGET in installed and os.name != "nt":
        for directory in (
            Path("/opt/homebrew/opt/llvm@22/bin"),
            Path("/opt/homebrew/opt/llvm/bin"),
            Path("/opt/homebrew/opt/llvm@20/bin"),
        ):
            if directory.is_dir():
                env["PATH"] = str(directory) + os.pathsep + env.get("PATH", "")
                break
    can_check_windows = WINDOWS_TARGET in installed
    if can_check_windows and os.name != "nt" and not shutil.which("llvm-rc", path=env.get("PATH")):
        print("win-tauri-check: llvm-rc unavailable; falling back to host target", flush=True)
        can_check_windows = False
    if can_check_windows:
        target = WINDOWS_TARGET
    else:
        version = subprocess.check_output(["rustc", "-vV"], text=True)
        target = next(
            line.removeprefix("host: ")
            for line in version.splitlines()
            if line.startswith("host: ")
        )
    print(f"win-tauri-check: cargo check target {target}", flush=True)
    ensure_icons()
    return subprocess.run(["cargo", "check", "--target", target], cwd=SHELL, env=env).returncode


if __name__ == "__main__":
    raise SystemExit(main())
