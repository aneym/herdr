#!/usr/bin/env python3
"""Live priority, parked folding and agent-request proof, in Cua Space only.

Run: HERDR_SHELL_SPACE=1 python3 scripts/check_priority_space.py
Uses the real lab CLI/API and the Shell's TestHook sidebar dump; no AX or osascript, which the
guest refuses (-609). The request dot is read from the agent row's dump ("dot:accent" in its face,
the row's one blue dot) and confirmed in light and dark 2x shots at the face's drawn frame.
Builds this worktree under the Studio build lock; never uses the live server.
"""
import json
import os
import pathlib
import shlex
import shutil
import subprocess
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_priority_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
TARGET = pathlib.Path(os.environ.get("HERDR_PROOF_TARGET", str(REPO.parent / ".target-space-proof")))
os.environ["SHELL_LAB"] = "shellspike-pr"
os.environ["SHELL_LAB_DIR"] = os.path.expanduser("~/.cache/herdr-build/pr")
os.environ.setdefault("HERDR_SPACE_OWNER", "space-proof-priority")
os.environ["HERDR_SHELL_BIN"] = str(TARGET / "debug/herdr")
os.environ["HERDR_SHELL_APP"] = str(ROOT / ".build/release/HerdrShell")
import scenario as S  # noqa: E402

# scenario's default LAB does not honor lab.py's short-directory override.
S.LAB = os.environ["SHELL_LAB_DIR"]
S.STATE = str(pathlib.Path(S.LAB) / "state.json")
S.OUT = str(ROOT / "checks/PRIORITY-SPACE.txt")
_original_lab = S.lab


def debug_lab(*args):
    output = _original_lab(*args)
    if args == ("env",):
        output = output.replace("/.config/herdr/sessions/", "/.config/herdr-dev/sessions/")
    return output


# The debug binary resolves the session in herdr-dev; the bridge must use
# that actual socket rather than lab.py's release-only socket spelling.
S.lab = debug_lab
lines = []
failed = False


def check(name, condition, detail=""):
    global failed
    line = f"[{'PASS' if condition else 'FAIL'}] {name}"
    if not condition and detail:
        line += " (" + detail + ")"
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failed = True
        raise AssertionError(line)


def build():
    lock = pathlib.Path.home() / ".agent-rails/locks/studio-build"
    lock.parent.mkdir(parents=True, exist_ok=True)
    while True:
        try:
            lock.mkdir()
            break
        except FileExistsError:
            print("waiting for studio-build lock", flush=True)
            time.sleep(10)
    try:
        (lock / "owner").write_text(f"space-proof-priority {int(time.time())}\n")
        env = dict(os.environ, CC="/usr/bin/cc",
                   CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="/usr/bin/cc",
                   ZIG=os.path.expanduser("~/.cache/herdr-zig/dl/zig-aarch64-macos-0.16.0/zig"),
                   CARGO_TARGET_DIR=str(TARGET))
        subprocess.run(["cargo", "build", "--bin", "herdr"], cwd=REPO, env=env, check=True)
        # Reuse the pinned, prebuilt Ghostty archive without copying/editing any
        # tracked file. Package.swift already supplies the remaining link flags.
        ghostty = pathlib.Path("/Volumes/StudioExt/repos/herdr-shell-spikes/vendor/ghostty/macos/GhosttyKit.xcframework/macos-arm64")
        check("prebuilt Ghostty archive available", (ghostty / "libghostty-internal.a").is_file())
        subprocess.run(["swift", "build", "-c", "release", "-Xlinker", "-L" + str(ghostty)],
                       cwd=ROOT, check=True)
        check("worktree herdr and Shell built", pathlib.Path(os.environ["HERDR_SHELL_BIN"]).is_file()
              and pathlib.Path(os.environ["HERDR_SHELL_APP"]).is_file())
    finally:
        shutil.rmtree(lock)


def lab_run(*args):
    env = dict(line.split("=", 1) for line in S.lab("env").splitlines() if "=" in line)
    binary = pathlib.Path(env["PATH"].split(":")[0]) / "herdr"
    result = subprocess.run([str(binary), "--session", S.NAME, *args], env=env,
                            capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise RuntimeError(f"lab CLI {args}: exit {result.returncode}: {result.stdout} {result.stderr}")
    return result.stdout


def api(*args):
    reply = json.loads(lab_run(*args))
    if "error" in reply:
        raise RuntimeError(json.dumps(reply))
    return reply["result"]


def wait(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            break
        time.sleep(0.2)
    return state


def rows(state):
    return [row.split("|") for row in state.get("spaces_rows", [])]


def ids(state):
    return [r[1] for r in rows(state)]


def space_labels(state):
    return [r[6] for r in rows(state) if r[0] == "space"]


def agent_fields(state, tab):
    return next((r[13:] for r in rows(state) if r[1] == "agent:" + tab), None)


def rgb(hex_color):
    return tuple(int(hex_color[i:i + 2], 16) for i in (1, 3, 5))


def frame(text):
    return [float(v) for v in text.replace("{", "").replace("}", "").split(",")]


def dot_pixels(png, state, tab):
    """The lower-right quarter of the agent's drawn face, where its dot sits."""
    from PIL import Image
    img = Image.open(png).convert("RGB")
    scale = img.width / frame(state["window_frame"])[2]
    x, y, w, h = state["face_frames"]["face:agent:" + tab]
    return [img.getpixel((int(px), int(py))) for px in range(int((x + w / 2) * scale), int((x + w + 2) * scale))
            for py in range(int((y + h / 2) * scale), int((y + h + 2) * scale))]


def near(pixels, color, tolerance=40):
    return any(sum(abs(a - b) for a, b in zip(p, color)) <= tolerance for p in pixels)


def shot(name, scale=None):
    path = ROOT / f"checks/PRIORITY-SPACE-{name}.png"
    if path.exists():
        path.unlink()
    S.cmd({"cmd": "shot", "out": str(path)} | ({"scale": scale} if scale else {}))
    deadline = time.monotonic() + 10
    while not path.is_file() and time.monotonic() < deadline:
        time.sleep(0.2)
    check(f"{name} screenshot written", path.is_file() and path.stat().st_size > 0, str(path))
    print(str(path), flush=True)
    return str(path)


def main():
    build()
    S.app("stop")
    S.lab("down")
    S.lab("up")
    config = pathlib.Path(S.LAB) / "h/.config/herdr/config.toml"
    check("config is isolated in short lab XDG", config.is_file()
          and len(str(config.parent / "sessions/shellspike-pr/herdr.sock.client.sock").encode()) < 104)
    # Debug herdr namespaces its config as herdr-dev, even though lab.py
    # seeds herdr/config.toml. Both stay under this lab's XDG_CONFIG_HOME.
    dev_config = config.parent.parent / "herdr-dev/config.toml"
    dev_config.parent.mkdir(parents=True, exist_ok=True)
    dev_config.write_text(config.read_text())
    with dev_config.open("a") as f:
        f.write('\n[ui.sidebar.priority]\norder = ["recruiting", "factory", "tab:factory throughput", "herdr"]\nlast = ["rails"]\n')
    lab_run("server", "reload-config")
    old = api("workspace", "list")["workspaces"]
    spaces, rails_tabs = {}, {}
    agent = None
    for name in ("rails", "misc", "herdr", "factory", "recruiting"):
        made = api("workspace", "create", "--label", name, "--no-focus")
        spaces[name] = made["workspace"]["workspace_id"]
        tab = made["tab"]["tab_id"]
        if name == "rails":
            lab_run("tab", "rename", tab, "orchestrator")
            rails_tabs["orchestrator"] = tab
            for label in ("x build", "factory throughput"):
                rails_tabs[label] = api("tab", "create", "--workspace", spaces[name], "--label", label,
                                        "--no-focus")["tab"]["tab_id"]
        elif name == "recruiting":
            agent = tab
            lab_run("tab", "rename", tab, "priority agent")
            lab_run("tab", "set-role", tab, "agent")
    for workspace in old:
        lab_run("workspace", "close", workspace["workspace_id"])
    workspaces = api("workspace", "list")["workspaces"]
    for name, rank in (("recruiting", 0), ("factory", 1), ("herdr", 3), ("misc", 4), ("rails", 5)):
        item = next(w for w in workspaces if w["workspace_id"] == spaces[name])
        check(f"API {name} sort_rank {rank}", item.get("sort_rank") == rank, json.dumps(item))
        if name == "rails":
            check("API rails parked true", item.get("parked") is True, json.dumps(item))
    tabs = api("tab", "list", "--workspace", spaces["rails"])["tabs"]
    for label, rank in (("factory throughput", 2), ("orchestrator", 5), ("x build", 5)):
        item = next(t for t in tabs if t["tab_id"] == rails_tabs[label])
        check(f"API rails tab {label} sort_rank {rank}", item.get("sort_rank") == rank, json.dumps(item))
    agent_tab = next(t for t in api("tab", "list", "--workspace", spaces["recruiting"])["tabs"] if t["tab_id"] == agent)
    check("API agent role is pinned", agent_tab.get("role") == "agent" and agent_tab.get("pin_index") is not None,
          json.dumps(agent_tab))
    pane = next(p["pane_id"] for p in api("pane", "list", "--workspace", spaces["recruiting"])["panes"]
                if p["tab_id"] == agent)
    S.app("start")
    S.cmd({"cmd": "activate"})
    expected = ["recruiting", "factory", "herdr", "misc", "rails"]
    state = wait(lambda s: space_labels(s) == expected and "agent:" + agent in ids(s))
    check("Shell spaces sorted recruiting, factory, herdr, misc, rails", space_labels(state) == expected,
          json.dumps(state.get("spaces_rows")))
    check("AGENTS section shows agent-role pin", "agentpins" in ids(state) and "agent:" + agent in ids(state))
    railrow = next(r for r in rows(state) if r[1] == "space:" + spaces["rails"])
    check("rails folded by default with no rails tabs shown", railrow[3] == "closed"
          and all("tab:" + t not in ids(state) for t in rails_tabs.values()), json.dumps(state.get("spaces_rows")))
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        state = wait(lambda s: s.get("theme", {}).get("effective") == mode)
        check(mode + " appearance applied", state.get("theme", {}).get("effective") == mode)
        check(mode + " sorted sidebar retains folded rails", space_labels(state) == expected
              and all("tab:" + t not in ids(state) for t in rails_tabs.values()))
        shot(mode)
    S.cmd({"cmd": "spaces_click", "row": "space:" + spaces["rails"], "part": "chevron"})
    state = wait(lambda s: all("tab:" + t in ids(s) for t in rails_tabs.values()))
    shown = [r[1] for r in rows(state) if r[1] in {"tab:" + t for t in rails_tabs.values()}]
    check("expanded rails shows factory throughput first", shown == ["tab:" + rails_tabs[n]
          for n in ("factory throughput", "orchestrator", "x build")], json.dumps(state.get("spaces_rows")))
    S.cmd({"cmd": "spaces_click", "row": "space:" + spaces["rails"], "part": "chevron"})
    state = wait(lambda s: all("tab:" + t not in ids(s) for t in rails_tabs.values()))
    check("rails refolded", all("tab:" + t not in ids(state) for t in rails_tabs.values()))
    help_output = lab_run("pane", "report-metadata", "--help")
    check("CLI supports exact --clear-token flag", "--clear-token" in help_output, help_output)
    state = S.state()
    check("agent row has a face and no dot before any request",
          agent_fields(state, agent) is not None and agent_fields(state, agent)[:1] != []
          and agent_fields(state, agent)[0].startswith("face:")
          and not any(f.startswith(("dot:", "request:")) for f in agent_fields(state, agent)),
          json.dumps(agent_fields(state, agent)))
    lab_run("pane", "report-metadata", pane, "--source", "agent-request", "--token", "request=req-42")
    state = wait(lambda s: "dot:accent" in (agent_fields(s, agent) or []))
    fields = agent_fields(state, agent) or []
    check("open request turns the agent's face dot accent", "dot:accent" in fields, json.dumps(fields))
    check("one blue dot: no trailing request dot beside the face", not any(f.startswith("request:") for f in fields),
          json.dumps(fields))
    check("request dot sidebar retains sorted spaces and folded rails", space_labels(state) == expected
          and all("tab:" + t not in ids(state) for t in rails_tabs.values()))
    for phase in ("request", "resolved"):
        if phase == "resolved":
            lab_run("pane", "report-metadata", pane, "--source", "agent-request", "--clear-token", "request")
            state = wait(lambda s: not any(f.startswith(("dot:", "request:")) for f in (agent_fields(s, agent) or ["dot:"])))
            fields = agent_fields(state, agent) or []
            check("resolving the request removes the dot from the dump",
                  fields[:1] != [] and not any(f.startswith(("dot:", "request:")) for f in fields), json.dumps(fields))
        for mode in ("light", "dark"):
            S.cmd({"cmd": "appearance", "mode": mode})
            state = wait(lambda s: s.get("theme", {}).get("effective") == mode)
            check(f"{phase} {mode} appearance applied", state.get("theme", {}).get("effective") == mode)
            time.sleep(0.5)
            state = S.state()
            png = shot(f"{phase}-{mode}", scale=2)
            accent = rgb(state["face_dots"]["blocked"])
            drawn = near(dot_pixels(png, state, agent), accent)
            check(f"{phase} {mode}: the face dot is " + ("drawn accent" if phase == "request" else "gone"),
                  drawn == (phase == "request"))

if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        if not failed:
            failed = True
            line = f"[FAIL] scenario completed ({exc})"
            print(line, flush=True)
            lines.append(line)
    finally:
        try:
            S.app("stop")
            S.lab("down")
        except (Exception, SystemExit) as exc:
            failed = True
            lines.append(f"[FAIL] lab/Space cleanup ({exc})")
        pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
        pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    raise SystemExit(1 if failed else 0)
