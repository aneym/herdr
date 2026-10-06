#!/usr/bin/env python3
"""PC build/install/run harness for the Windows Herdr Shell.

Runs on Studio (macOS), stdlib only. All real work happens on the PC over
`ssh pc`; PowerShell helpers are scp'd to C:\\Users\\aneym\\winshell\\scripts.

Subcommands: sync, build, install, run, ctl, shot, status.
"""

import argparse
import base64
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent          # windows/HerdrShell/scripts
SHELL = HERE.parent                              # windows/HerdrShell
REPO = SHELL.parents[1]                          # worktree root

W = "C:/Users/aneym/winshell"
R_SRC = f"{W}/src"
R_SCRIPTS = f"{W}/scripts"
R_STAGE = f"{W}/stage"
R_OUT = f"{W}/out"
R_SHOTS = f"{W}/shots"
R_CACHE = f"{W}/cache"

PS = "powershell -NoProfile -ExecutionPolicy Bypass"


def remote(cmd, input_data=None, stream=False):
    """Run cmd on the PC via ssh. Returns (rc, stdout) or streams output."""
    if stream:
        return subprocess.run(["ssh", "pc", cmd]).returncode, ""
    p = subprocess.run(
        ["ssh", "pc", cmd],
        input=input_data,
        capture_output=True,
        text=isinstance(input_data, str) or input_data is None,
    )
    out = p.stdout if isinstance(p.stdout, str) else (p.stdout or b"").decode("utf-8", "replace")
    if p.returncode != 0 and p.stderr:
        err = p.stderr if isinstance(p.stderr, str) else p.stderr.decode("utf-8", "replace")
        print(err.strip(), file=sys.stderr)
    return p.returncode, out


def ps_file(name, *args, stream=False):
    argstr = " ".join(args)
    cmd = f"{PS} -File {R_SCRIPTS}/{name}"
    if argstr:
        cmd += " " + argstr
    return remote(cmd, stream=stream)


def scp_to(local, remote_path):
    p = subprocess.run(["scp", "-q", str(local), f"pc:{remote_path}"])
    return p.returncode


def scp_from(remote_path, local):
    p = subprocess.run(["scp", "-q", f"pc:{remote_path}", str(local)])
    return p.returncode


def bootstrap():
    remote(
        f"{PS} -Command \"New-Item -ItemType Directory -Force -Path "
        f"'{R_SCRIPTS}','{R_OUT}','{R_SHOTS}','{R_STAGE}','{R_CACHE}' | Out-Null\""
    )
    for f in sorted(HERE.glob("*.ps1")):
        rc = scp_to(f, f"{R_SCRIPTS}/{f.name}")
        if rc != 0:
            print(f"scp failed for {f.name}", file=sys.stderr)
            sys.exit(1)


def guard(quiet=False):
    """Returns (game_running: bool, payload: dict). Prints guard JSON."""
    rc, out = ps_file("game_guard.ps1")
    if not quiet:
        print(out.strip())
    try:
        data = json.loads(out.strip())
    except json.JSONDecodeError:
        data = {"game": rc == 3, "procs": [], "idle_s": None}
    return rc == 3, data


def git_sha():
    sha = subprocess.run(
        ["git", "-C", str(REPO), "rev-parse", "HEAD"],
        capture_output=True, text=True,
    ).stdout.strip()
    dirty = subprocess.run(
        ["git", "-C", str(REPO), "status", "--porcelain", "--", "windows/HerdrShell"],
        capture_output=True, text=True,
    ).stdout.strip()
    return sha + ("+dirty" if dirty else "")


def cmd_sync(_args):
    bootstrap()
    remote(
        f"{PS} -Command \"if (Test-Path '{R_STAGE}') {{ Remove-Item -Recurse -Force '{R_STAGE}' }}; "
        f"New-Item -ItemType Directory -Force -Path '{R_STAGE}' | Out-Null\""
    )
    excl = []
    for pat in (
        "*/node_modules", "*/node_modules/*", "*/dist", "*/dist/*",
        "*/target", "*/target/*", "*/.vite", "*/.vite/*",
        "*.exe", "*/icons/*.png", "*/icons/*.ico", "*/icons/*.icns",
        ".DS_Store", "*/.DS_Store", "._*", "*/._*",
    ):
        excl += ["--exclude", pat]
    tar = subprocess.Popen(
        ["tar", "-cf", "-", "-C", str(REPO)] + excl + ["windows/HerdrShell"],
        stdout=subprocess.PIPE,
        env={**os.environ, "COPYFILE_DISABLE": "1"},
    )
    rc, out = _untar(tar)
    if rc != 0:
        print("remote untar failed", file=sys.stderr)
        sys.exit(rc)
    rc, out = ps_file("apply_sync.ps1")
    if rc != 0:
        print(out, file=sys.stderr)
        sys.exit(rc)
    print(f"synced windows/HerdrShell -> {R_SRC}")


def _untar(tar_proc):
    p = subprocess.Popen(
        ["ssh", "pc", f"tar -xf - -C {R_STAGE}"],
        stdin=tar_proc.stdout,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    tar_proc.stdout.close()
    out = p.communicate()[0].decode("utf-8", "replace")
    tar_rc = tar_proc.wait()
    if out.strip():
        print(out.strip())
    return p.returncode or tar_rc, out


def cmd_build(_args):
    bootstrap()
    game, _data = guard()
    if game:
        print("game running; not building (exit 75)", file=sys.stderr)
        sys.exit(75)
    cmd_sync(_args)
    sha = git_sha()
    rc, _ = ps_file("build.ps1", "-Sha", sha, stream=True)
    if rc == 75:
        print("build aborted: a game started mid-build", file=sys.stderr)
        sys.exit(75)
    sys.exit(rc)


def cmd_install(args):
    bootstrap()
    argv = ["-Sha", args.sha] if args.sha else []
    rc, out = ps_file("install.ps1", *argv)
    print(out.strip())
    sys.exit(rc)


def ctl_send(obj):
    b64 = base64.b64encode(json.dumps(obj).encode()).decode()
    rc, out = ps_file("ctl.ps1", "-JsonB64", b64)
    return rc, out.strip()


def cmd_ctl(args):
    bootstrap()
    try:
        obj = json.loads(args.json)
    except json.JSONDecodeError as e:
        print(f"bad json: {e}", file=sys.stderr)
        sys.exit(2)
    rc, out = ctl_send(obj)
    print(out)
    sys.exit(rc)


def cmd_shot(args):
    bootstrap()
    remote(f"{PS} -Command \"New-Item -ItemType Directory -Force -Path '{R_SHOTS}' | Out-Null\"")
    rpath = f"{R_SHOTS}/shot-{time.strftime('%Y%m%d-%H%M%S')}.png".replace("/", "\\")
    rc, out = ctl_send({"cmd": "shot", "out": rpath})
    print(out)
    if rc != 0:
        sys.exit(rc)
    try:
        ok = json.loads(out).get("ok")
    except json.JSONDecodeError:
        ok = False
    if not ok:
        sys.exit(1)
    rc = scp_from(rpath.replace("\\", "/"), args.out)
    if rc != 0:
        print("scp back failed", file=sys.stderr)
        sys.exit(rc)
    print(f"saved {args.out}")


def refresh_idle():
    rc, out = ps_file("idle_refresh.ps1")
    try:
        return json.loads(out.strip())
    except json.JSONDecodeError:
        return {}


def cmd_run(args):
    bootstrap()
    game, _ = guard()
    if game:
        print("game running; not launching (exit 75)", file=sys.stderr)
        sys.exit(75)
    idle = refresh_idle()
    idle_s = idle.get("idle_s")
    # --test-window opens off every monitor without focus, so it may run while Alex is active.
    if (idle_s is None or idle_s < 300) and not args.test_window:
        if not args.force_idle:
            print(f"idle_s={idle_s} (<300 or unknown); not launching (exit 75)", file=sys.stderr)
            sys.exit(75)
    rc, out = ps_file("status.ps1")
    exe = None
    try:
        exe = json.loads(out.strip()).get("exe")
    except json.JSONDecodeError:
        pass
    if not exe:
        print("HerdrShell.exe not installed; run install first", file=sys.stderr)
        sys.exit(1)
    launch = ["launch.ps1", "-Exe", f'"{exe}"']
    if args.test_window:
        launch.append("-TestWindow")
    rc, out = ps_file(*launch)
    print(out.strip())
    sys.exit(rc)


def cmd_status(_args):
    bootstrap()
    game, gdata = guard(quiet=True)
    idle = refresh_idle()
    rc, out = ps_file("status.ps1")
    app = {}
    try:
        app = json.loads(out.strip())
    except json.JSONDecodeError:
        pass
    gdata["idle_s"] = idle.get("idle_s", gdata.get("idle_s"))
    print(json.dumps({"game": gdata, "idle": idle, "app": app}, indent=1))
    sys.exit(0)


def main():
    ap = argparse.ArgumentParser(description="Herdr Shell PC harness")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("sync", help="tar windows/HerdrShell to the PC")
    p.add_argument("--src", default=str(SHELL), help="source dir (unused; tree is fixed)")
    p.set_defaults(fn=cmd_sync)

    p = sub.add_parser("build", help="guard + sync + tauri build --bundles nsis")
    p.set_defaults(fn=cmd_build)

    p = sub.add_parser("install", help="run the NSIS installer silently")
    p.add_argument("--sha", default="")
    p.set_defaults(fn=cmd_install)

    p = sub.add_parser("run", help="launch the installed app on Alex's desktop")
    p.add_argument("--force-idle", action="store_true")
    p.add_argument("--test-window", action="store_true")
    p.set_defaults(fn=cmd_run)

    p = sub.add_parser("ctl", help="send one JSON line to the control pipe")
    p.add_argument("json")
    p.set_defaults(fn=cmd_ctl)

    p = sub.add_parser("shot", help="screenshot the app window to a local PNG")
    p.add_argument("--out", required=True)
    p.set_defaults(fn=cmd_shot)

    p = sub.add_parser("status", help="game + idle + app status as JSON")
    p.set_defaults(fn=cmd_status)

    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
