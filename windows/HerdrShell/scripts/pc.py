#!/usr/bin/env python3
"""PC build/install/run harness for the Windows Herdr Shell.

Runs on Studio (macOS), stdlib only. All real work happens on the PC over
`ssh pc`; PowerShell helpers are scp'd to C:\\Users\\aneym\\winshell\\scripts.

Subcommands: sync, build, fetch, install, run, ctl, shot, status.

Windows App Control on the PC blocks cargo's freshly linked build scripts, so
the normal path builds on a GitHub Windows runner (windows-shell.yml):
`fetch --sha <sha> --dispatch` copies the built exe and installer to the PC and
`install --artifact <sha> --relaunch` swaps the exe in place.
"""

import argparse
import base64
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
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
R_SHARED_SHELL = "C:/Users/aneym/shell"
SYNC_PATHS = ("windows/HerdrShell", "shell")

PS = "powershell -NoProfile -ExecutionPolicy Bypass"
GH_REPO = "aneym/herdr"
WORKFLOW = "windows-shell.yml"


def remote(cmd, input_data=None, stream=False, timeout=None):
    """Run cmd on the PC via ssh. Returns (rc, stdout) or streams output."""
    if stream:
        return subprocess.run(["ssh", "pc", cmd]).returncode, ""
    p = subprocess.run(
        ["ssh", "pc", cmd],
        input=input_data,
        capture_output=True,
        text=isinstance(input_data, str) or input_data is None,
        timeout=timeout,
    )
    out = p.stdout if isinstance(p.stdout, str) else (p.stdout or b"").decode("utf-8", "replace")
    if p.returncode != 0 and p.stderr:
        err = p.stderr if isinstance(p.stderr, str) else p.stderr.decode("utf-8", "replace")
        print(err.strip(), file=sys.stderr)
    return p.returncode, out


class Gated(SystemExit):
    """A game runs on the PC: the helper did not act, and nothing after it may."""

    def __init__(self, name, reason=""):
        print(f"{reason.strip() or 'GATED'}; {name} not run (exit 75)", file=sys.stderr)
        super().__init__(75)


# Read-only probes; every other helper can touch the app or the PC's load.
UNGATED = {"game_guard.ps1", "status.ps1"}
GATED = 75


def ps_file(name, *args, stream=False, timeout=None):
    """Run one helper on the PC; all but UNGATED go through gated.ps1, which
    checks for a game in the same process and exits 75 before acting."""
    if name in UNGATED:
        cmd = " ".join([f"{PS} -File {R_SCRIPTS}/{name}", *args])
    else:
        b64 = base64.b64encode(json.dumps(list(args)).encode()).decode()
        cmd = f"{PS} -File {R_SCRIPTS}/gated.ps1 -Script {name} -ArgsB64 {b64}"
    rc, out = remote(cmd, stream=stream, timeout=timeout)
    if rc == GATED and name not in UNGATED:
        raise Gated(name, out)
    return rc, out


def scp_to(local, remote_path):
    p = subprocess.run(["scp", "-q", str(local), f"pc:{remote_path}"])
    return p.returncode


def scp_from(remote_path, local):
    p = subprocess.run(["scp", "-q", f"pc:{remote_path}", str(local)])
    return p.returncode


# First commit whose PC helpers keep every probe off Alex's desktop. Older
# checkouts would copy the flashing idle task back onto the PC.
DESKTOP_SAFE = "57faaaa8e0e7f31fb82717f77649dcf9186c9d0d"


def stale_checkout(repo=None):
    """True when repo is a git checkout that does not contain DESKTOP_SAFE.
    The launchd fanout runs an installed snapshot outside any checkout."""
    repo = repo or REPO
    inside = subprocess.run(["git", "-C", str(repo), "rev-parse", "--is-inside-work-tree"],
                            capture_output=True, text=True)
    if inside.returncode != 0:
        return False
    rc = subprocess.run(["git", "-C", str(repo), "merge-base", "--is-ancestor", DESKTOP_SAFE, "HEAD"],
                        capture_output=True, text=True).returncode
    return rc != 0


def bootstrap():
    if stale_checkout():
        print(f"{REPO} does not contain {DESKTOP_SAFE[:8]}; its PC helpers would flash a console "
              f"on Alex's desktop. Rebase first: git -C {REPO} fetch origin && "
              f"git -C {REPO} rebase origin/main", file=sys.stderr)
        sys.exit(3)
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


def git_sha(repo=REPO):
    sha = subprocess.run(
        ["git", "-C", str(repo), "rev-parse", "HEAD"],
        capture_output=True, text=True,
    ).stdout.strip()
    dirty = subprocess.run(
        ["git", "-C", str(repo), "status", "--porcelain", "--", "windows/HerdrShell"],
        capture_output=True, text=True,
    ).stdout.strip()
    return sha + ("+dirty" if dirty else "")


def cmd_sync(_args):
    repo = Path(_args.src).resolve()
    excl = []
    for pat in (
        "*/node_modules", "*/node_modules/*", "*/dist", "*/dist/*",
        "*/target", "*/target/*", "*/.vite", "*/.vite/*",
        "*.exe", "*/icons/*.png", "*/icons/*.ico", "*/icons/*.icns",
        ".DS_Store", "*/.DS_Store", "._*", "*/._*",
    ):
        # Exclude Windows build artifacts without filtering the shared shell tree.
        excl += ["--exclude", f"windows/HerdrShell/{pat}"]
        if pat.startswith("*/"):
            excl += ["--exclude", f"windows/HerdrShell/{pat[2:]}"]
        else:
            excl += ["--exclude", f"windows/HerdrShell/*/{pat}"]
    tar_cmd = ["tar", "-cf", "-", "-C", str(repo)] + excl + list(SYNC_PATHS)
    if getattr(_args, "dry_run", False):
        print(f"windows/HerdrShell -> {R_SRC}")
        print(f"shell -> {R_SHARED_SHELL}")
        payload = subprocess.run(
            tar_cmd, capture_output=True, env={**os.environ, "COPYFILE_DISABLE": "1"},
        )
        if payload.returncode != 0:
            print(payload.stderr.decode("utf-8", "replace"), file=sys.stderr)
            sys.exit(payload.returncode)
        listing = subprocess.run(["tar", "-tf", "-"], input=payload.stdout,
                                 capture_output=True)
        print(listing.stdout.decode("utf-8", "replace"), end="")
        if listing.returncode != 0:
            print(listing.stderr.decode("utf-8", "replace"), file=sys.stderr)
            sys.exit(listing.returncode)
        return
    bootstrap()
    remote(
        f"{PS} -Command \"if (Test-Path '{R_STAGE}') {{ Remove-Item -Recurse -Force '{R_STAGE}' }}; "
        f"New-Item -ItemType Directory -Force -Path '{R_STAGE}' | Out-Null\""
    )
    tar = subprocess.Popen(
        tar_cmd, stdout=subprocess.PIPE,
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
    print(f"synced windows/HerdrShell -> {R_SRC}; shell -> {R_SHARED_SHELL}")


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
    sha = git_sha(Path(_args.src).resolve())
    rc, _ = ps_file("build.ps1", "-Sha", sha, stream=True)
    if rc == 75:
        print("build aborted: a game started mid-build", file=sys.stderr)
        sys.exit(75)
    sys.exit(rc)


def gh(*args, timeout=120):
    p = subprocess.run(["gh", *args], capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout.strip()


def find_artifact_run(sha):
    """Run id of the newest unexpired herdr-shell-<sha> artifact, or None."""
    rc, out = gh("api", f"repos/{GH_REPO}/actions/artifacts?name=herdr-shell-{sha}&per_page=10",
                 "--jq", "[.artifacts[] | select(.expired | not) | .workflow_run.id][0] // empty")
    return out if rc == 0 and out else None


def full_sha(ref):
    # The launchd fanout runs an installed copy outside any checkout.
    if len(ref) == 40 and all(c in "0123456789abcdef" for c in ref):
        return ref
    out = subprocess.run(["git", "-C", str(REPO), "rev-parse", "--verify", f"{ref}^{{commit}}"],
                         capture_output=True, text=True)
    if out.returncode != 0:
        print(f"unknown commit {ref}", file=sys.stderr)
        sys.exit(2)
    return out.stdout.strip()


# A sha whose build failed this often is not dispatched again; fix the build first.
MAX_BUILD_FAILURES = 2
HEX64 = re.compile(r"^[0-9A-Fa-f]{64}$")


def build_runs(sha):
    """windows-shell.yml runs for sha, newest first (its run-name carries the sha)."""
    rc, out = gh("run", "list", "-R", GH_REPO, "-w", WORKFLOW, "-L", "100",
                 "--json", "databaseId,status,conclusion,displayTitle")
    if rc != 0:
        return None
    try:
        runs = json.loads(out)
    except ValueError:
        return None
    return [r for r in runs if r.get("displayTitle") == f"windows shell {sha}"]


def run_conclusion(run_id, deadline):
    while time.monotonic() < deadline:
        rc, out = gh("run", "view", str(run_id), "-R", GH_REPO, "--json", "status,conclusion")
        if rc == 0:
            try:
                r = json.loads(out)
            except ValueError:
                r = {}
            if r.get("status") == "completed":
                return r.get("conclusion")
        time.sleep(30)
    return None


def fail(msg, code=1):
    print(msg, file=sys.stderr)
    sys.exit(code)


def ensure_build(sha, dispatch):
    """Run id holding the herdr-shell-<sha> artifact. Waits on a running build for sha
    instead of starting another; exits 4 when the build failed."""
    run = find_artifact_run(sha)
    if run:
        return run
    runs = build_runs(sha)
    if runs is None:
        fail("cannot list windows-shell.yml runs")
    active = [r for r in runs if r.get("status") != "completed"]
    failed = [r for r in runs if r.get("status") == "completed" and r.get("conclusion") != "success"]
    if active:
        run_id = active[0]["databaseId"]
    elif not dispatch:
        fail(f"no herdr-shell-{sha} artifact (dispatch with --dispatch)")
    elif len(failed) >= MAX_BUILD_FAILURES:
        ids = ", ".join(str(r["databaseId"]) for r in failed)
        fail(f"{len(failed)} failed builds for {sha} (runs {ids}); not dispatching again", 4)
    else:
        seen = {r["databaseId"] for r in runs}
        rc, out = gh("workflow", "run", WORKFLOW, "-R", GH_REPO, "--ref", "main", "-f", f"ref={sha}")
        if rc != 0:
            fail(f"dispatch failed: {out}")
        run_id = None
        deadline = time.monotonic() + 120
        while run_id is None and time.monotonic() < deadline:
            time.sleep(5)
            fresh = [r for r in (build_runs(sha) or []) if r["databaseId"] not in seen]
            run_id = fresh[0]["databaseId"] if fresh else None
        if run_id is None:
            fail(f"dispatched {WORKFLOW} for {sha} but its run did not appear")
        print(f"dispatched {WORKFLOW} for {sha}")
    print(f"build run {run_id} for {sha}")
    conclusion = run_conclusion(run_id, time.monotonic() + 60 * 60)
    if conclusion != "success":
        fail(f"build run {run_id} for {sha} ended {conclusion or 'unfinished after 60 min'}", 4)
    for _ in range(10):
        run = find_artifact_run(sha)
        if run:
            return run
        time.sleep(3)
    fail(f"build run {run_id} succeeded but has no herdr-shell-{sha} artifact")


def verify_artifact(d, sha):
    """{file name: SHA-256} for a downloaded artifact; exits unless build-sha.txt names
    sha and SHA256SUMS lists exactly the exe and installer with matching hashes."""
    stamp = d / "build-sha.txt"
    if not stamp.is_file() or stamp.read_text().strip() != sha:
        fail(f"artifact build-sha.txt is missing or does not name {sha}")
    sums = {}
    sums_file = d / "SHA256SUMS"
    for line in (sums_file.read_text().splitlines() if sums_file.is_file() else []):
        parts = line.split()
        if len(parts) != 2 or not HEX64.match(parts[0]):
            fail(f"malformed SHA256SUMS line: {line!r}")
        sums[parts[1]] = parts[0].upper()
    want = {"HerdrShell.exe", f"HerdrShell-setup-{sha}.exe"}
    if set(sums) != want:
        fail(f"SHA256SUMS lists {sorted(sums)}, expected {sorted(want)}")
    for name, digest in sums.items():
        f = d / name
        if not f.is_file() or hashlib.sha256(f.read_bytes()).hexdigest().upper() != digest:
            fail(f"checksum mismatch for {name}")
    return sums


def remote_ps(script):
    """Run a short PowerShell script on the PC in the ssh session (no desktop)."""
    enc = base64.b64encode(script.encode("utf-16-le")).decode()
    return remote(f"{PS} -EncodedCommand {enc}")


def push_verified(local, name, digest):
    """scp to <name>.part, check the hash on the PC, then rename into place."""
    part, final = f"{R_OUT}/{name}.part", f"{R_OUT}/{name}"
    if scp_to(local, part) != 0:
        fail(f"scp failed for {name}")
    rc, _ = remote_ps(
        f"$p = '{part}'; if ((Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash -ne '{digest}') "
        f"{{ Remove-Item -LiteralPath $p -Force; exit 9 }}; Move-Item -LiteralPath $p -Destination '{final}' -Force")
    if rc != 0:
        fail(f"{name} arrived corrupted on the PC (rc {rc}); removed")


def cmd_fetch(args):
    sha = full_sha(args.sha)
    run = ensure_build(sha, args.dispatch)
    with tempfile.TemporaryDirectory(prefix="herdr-shell-") as tmp:
        rc, out = gh("run", "download", run, "-R", GH_REPO, "-n", f"herdr-shell-{sha}", "-D", tmp,
                     timeout=600)
        if rc != 0:
            fail(f"download failed: {out}")
        d = Path(tmp)
        sums = verify_artifact(d, sha)
        bootstrap()
        push_verified(d / f"HerdrShell-setup-{sha}.exe", f"HerdrShell-setup-{sha}.exe",
                      sums[f"HerdrShell-setup-{sha}.exe"])
        push_verified(d / "HerdrShell.exe", f"HerdrShell-{sha}.exe", sums["HerdrShell.exe"])
        # Written last: install_copy.ps1 trusts the exe only through this file.
        rc, _ = remote_ps(f"Set-Content -LiteralPath '{R_OUT}/HerdrShell-{sha}.sha256' "
                          f"-Value '{sums['HerdrShell.exe']}' -Encoding ASCII")
        if rc != 0:
            fail("could not record the exe checksum on the PC")
    print(f"fetched {sha} (run {run}) -> {R_OUT}")


def wait_ui(seconds):
    """(ui ok with machine up, summary) from the control pipe within seconds."""
    deadline = time.monotonic() + seconds
    summary = {"machine_state": "unavailable", "rows": 0, "panes": 0}
    while time.monotonic() < deadline:
        try:
            rc, reply = ctl_send({"cmd": "ui"}, timeout=deadline - time.monotonic())
            ui = json.loads(reply, strict=False)
            summary = {"machine_state": ui.get("machine", {}).get("state"),
                       "rows": len(ui.get("rows", [])), "panes": len(ui.get("panes", []))}
            if rc == 0 and ui.get("ok") is True and summary["machine_state"] == "up":
                return True, summary
        except (subprocess.TimeoutExpired, ValueError, TypeError, AttributeError):
            pass
        remaining = deadline - time.monotonic()
        if remaining > 0:
            time.sleep(min(1, remaining))
    return False, summary


def running_commit(seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            rc, pong = ctl_send({"cmd": "ping"}, timeout=max(1, deadline - time.monotonic()))
            commit = json.loads(pong).get("commit")
            if rc == 0 and commit:
                return commit
        except (subprocess.TimeoutExpired, ValueError, TypeError, AttributeError):
            pass
        time.sleep(1)
    return None


def cmd_install_artifact(args):
    """install_copy.ps1 swaps the exe; pc.py checks the commit and rolls back on a mismatch."""
    want = full_sha(args.artifact)
    rc, out = ps_file("install_copy.ps1", "-Sha", want, *(["-Relaunch"] if args.relaunch else []))
    print(out.strip())
    if rc == 76:
        fail("game started; install deferred (exit 75)", GATED)
    if rc != 0:
        sys.exit(rc)
    if not args.relaunch:
        # -Check reads the commit compiled into the installed exe and rolls back on a mismatch.
        rc, out = ps_file("install_copy.ps1", "-Sha", want, "-Check")
        print(out.strip())
        sys.exit(1 if rc else 0)
    commit = running_commit(60)
    up, summary = wait_ui(60) if commit == want else (False, {})
    summary["commit"] = commit
    print(json.dumps(summary))
    if commit != want or not up:
        why = f"running commit is {commit}, not {want}" if commit != want else \
            "UI did not report machine up within 60 s"
        print(f"{why}; rolling back", file=sys.stderr)
        _rc, out = ps_file("install_copy.ps1", "-Sha", want, "-Rollback", "-Relaunch")
        print(out.strip())
        sys.exit(1)
    # Verified only after the commit and the UI health check both pass.
    rc, out = ps_file("install_copy.ps1", "-Sha", want, "-MarkVerified")
    print(out.strip())
    if rc != 0:
        _rc, out = ps_file("install_copy.ps1", "-Sha", want, "-Rollback", "-Relaunch")
        print(out.strip())
        fail("install post-check failed: could not mark the installed exe verified; rolled back")
    sys.exit(0)


def cmd_install(args):
    bootstrap()
    if getattr(args, "artifact", ""):
        cmd_install_artifact(args)
    argv = ["-Sha", args.sha] if args.sha else []
    rc, out = ps_file("install.ps1", *argv)
    print(out.strip())
    if rc != 0 or not args.relaunch:
        sys.exit(rc)
    rc = launch_app(argparse.Namespace(force_idle=True, test_window=False))
    if rc != 0:
        sys.exit(rc)
    up, summary = wait_ui(60)
    print(json.dumps(summary))
    if not up:
        fail("install post-check failed: UI did not report machine up within 60 s")
    sys.exit(0)


def ctl_send(obj, timeout=None, test_window=False):
    b64 = base64.b64encode(json.dumps(obj).encode()).decode()
    switches = ["-Test"] if test_window else []
    rc, out = ps_file("ctl.ps1", "-JsonB64", b64, *switches, timeout=timeout)
    return rc, out.strip()


def cmd_ctl(args):
    try:
        obj = json.loads(args.json)
    except json.JSONDecodeError as e:
        print(f"bad json: {e}", file=sys.stderr)
        sys.exit(2)
    if isinstance(obj, dict) and obj.get("cmd") in {"open_detail", "row_menu", "paste_image", "drop_paths", "click", "hover"} and not getattr(args, "test_window", False):
        print("command requires --test-window", file=sys.stderr)
        sys.exit(2)
    bootstrap()
    rc, out = ctl_send(obj, test_window=getattr(args, "test_window", False))
    print(out)
    sys.exit(rc)


def cmd_shot(args):
    bootstrap()
    remote(f"{PS} -Command \"New-Item -ItemType Directory -Force -Path '{R_SHOTS}' | Out-Null\"")
    rpath = f"{R_SHOTS}/shot-{time.strftime('%Y%m%d-%H%M%S')}.png".replace("/", "\\")
    rc, out = ctl_send({"cmd": "shot", "out": rpath}, test_window=getattr(args, "test_window", False))
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
    # Gated: HerdrShellIdle runs in Alex's session, so a game that starts after an
    # earlier check must still stop it. A refusal means no idle reading.
    try:
        rc, out = ps_file("idle_refresh.ps1")
    except Gated:
        return {}
    try:
        return json.loads(out.strip())
    except json.JSONDecodeError:
        return {}


def cmd_run(args):
    sys.exit(launch_app(args))


def launch_app(args):
    bootstrap()
    game, _ = guard()
    if game:
        print("game running; not launching (exit 75)", file=sys.stderr)
        sys.exit(75)
    # --test-window opens off every monitor without focus, so it may run while Alex is active.
    if not args.test_window and not args.force_idle:
        idle_s = refresh_idle().get("idle_s")
        if idle_s is None or idle_s < 300:
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
    launch = ["launch.ps1", "-Exe", exe]
    if args.test_window:
        launch.append("-TestWindow")
    rc, out = ps_file(*launch)
    print(out.strip())
    return rc


def cmd_status(args):
    bootstrap()
    game, gdata = guard(quiet=True)
    # The idle probe runs a task in Alex's session; never during a game, and
    # only on request, so polls cannot touch his desktop.
    idle = refresh_idle() if getattr(args, "idle", False) and not game else {}
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

    p = sub.add_parser("sync", help="tar windows/HerdrShell and shared shell to the PC")
    p.add_argument("--src", default=str(REPO), help="source repository directory")
    p.add_argument("--dry-run", action="store_true",
                   help="list the sync archive and destinations locally; never contact the PC")
    p.set_defaults(fn=cmd_sync)

    p = sub.add_parser("build", help="guard + sync + tauri build --bundles nsis")
    p.add_argument("--src", default=str(REPO), help="source repository directory")
    p.set_defaults(fn=cmd_build)

    p = sub.add_parser("fetch", help="copy a GitHub-built exe and installer to the PC")
    p.add_argument("--sha", default="origin/main", help="commit to fetch (default origin/main)")
    p.add_argument("--dispatch", action="store_true", help="run windows-shell.yml when no artifact exists")
    p.set_defaults(fn=cmd_fetch)

    p = sub.add_parser("install", help="run the NSIS installer silently, or swap in a fetched exe")
    p.add_argument("--sha", default="")
    p.add_argument("--artifact", default="", help="commit fetched with `fetch`; copies the exe, no installer")
    p.add_argument("--relaunch", action="store_true", help="launch and verify the UI after installation")
    p.set_defaults(fn=cmd_install)

    p = sub.add_parser("run", help="launch the installed app on Alex's desktop")
    p.add_argument("--force-idle", action="store_true")
    p.add_argument("--test-window", action="store_true")
    p.set_defaults(fn=cmd_run)

    p = sub.add_parser("ctl", help="send one JSON line to the control pipe")
    p.add_argument("json")
    p.add_argument("--test-window", action="store_true", help="target the isolated test window pipe")
    p.set_defaults(fn=cmd_ctl)

    p = sub.add_parser("shot", help="screenshot the app window to a local PNG")
    p.add_argument("--out", required=True)
    p.add_argument("--test-window", action="store_true", help="target the isolated test window pipe")
    p.set_defaults(fn=cmd_shot)

    p = sub.add_parser("status", help="game + app status as JSON (idle with --idle)")
    p.add_argument("--idle", action="store_true",
                   help="also probe Alex's idle time (runs a task in his session; skipped during a game)")
    p.set_defaults(fn=cmd_status)

    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
