#!/usr/bin/env python3
"""AGENTS rows show each agent's face in the Cua Space: its agent.json picture, else its initial.

Run: HERDR_SHELL_SPACE=1 python3 scripts/check_agent_faces_space.py.
The static section check owns the row contract; this one covers the live path it cannot reach:
agent.json files on disk, matched by the server's real pane ids, reloaded when a card is added or
removed, and drawn by the running app: the fetched picture, the initials and the status dots are
read back from the screenshot's pixels at each face's drawn frame.
Writes checks/AGENT-FACES-SPACE.txt and light/dark shots; never launch on the host.
"""
import json
import os
import pathlib
import shlex
import shutil
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_agent_faces_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-af"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/AGENT-FACES-SPACE.txt")
AGENTS = pathlib.Path(S.LAB) / "agents"
PICTURE = "https://avatars.githubusercontent.com/u/9919?s=64&v=4"
lines, failures = [], []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.2)
    return state


def rgb(hex_color):
    return tuple(int(hex_color[i:i + 2], 16) for i in (1, 3, 5))


def face_pixels(png, state, tab):
    """The face's drawn pixels, and those of its lower-right quarter where the dot sits."""
    from PIL import Image
    img = Image.open(png).convert("RGB")
    width = float(state["window_frame"].strip("{}").replace("{", "").replace("}", "").split(",")[2])
    scale = img.width / width
    x, y, w, h = state["face_frames"]["face:agent:" + tab]
    box = [img.getpixel((int(px), int(py))) for px in range(int(x * scale), int((x + w) * scale))
           for py in range(int(y * scale), int((y + h) * scale))]
    quarter = [img.getpixel((int(px), int(py))) for px in range(int((x + w / 2) * scale), int((x + w + 2) * scale))
               for py in range(int((y + h / 2) * scale), int((y + h + 2) * scale))]
    return box, quarter


def near(pixels, color, tolerance=40):
    return any(sum(abs(a - b) for a, b in zip(p, color)) <= tolerance for p in pixels)


def dark_share(pixels):
    return sum(1 for p in pixels if sum(p) / 3 < 60) / max(1, len(pixels))


def faces(state):
    out = {}
    for row in state.get("spaces_rows", []):
        fields = row.split("|")
        if fields[1].startswith("agent:"):
            out[fields[1][len("agent:"):]] = next((f for f in fields if f.startswith("face:")), None)
    return out


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "agents", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws = made["workspace"]["workspace_id"]
    recruiter = made["tab"]["tab_id"]
    S.lab("herdr", "tab", "rename", recruiter, "Recruiter")
    frank, content, home = [api("tab", "create", "--workspace", ws, "--label", name, "--no-focus")["tab"]["tab_id"]
                            for name in ("Frank", "Content", "Home")]
    for tab in (recruiter, frank, content, home):
        S.lab("herdr", "tab", "set-role", tab, "agent")
    pane = {p["tab_id"]: p["pane_id"] for p in api("pane", "list")["panes"]}
    shutil.rmtree(AGENTS, ignore_errors=True)
    cards = {"recruiter": {"name": "recruiter", "pane": pane[recruiter]},
             "frank": {"name": "frank", "pane": pane[frank], "avatar_url": PICTURE},
             "home": {"name": "home", "pane": pane[home]}}
    for name, card in cards.items():
        (AGENTS / name).mkdir(parents=True)
        (AGENTS / name / "agent.json").write_text(json.dumps(card))
    os.environ["HERDR_AGENTS_DIR"] = str(AGENTS)
    S.lab("herdr", "pane", "report-agent", pane[recruiter], "--source", "spike", "--agent", "claude", "--state", "working")
    S.lab("herdr", "pane", "report-agent", pane[home], "--source", "spike", "--agent", "claude", "--state", "blocked")
    S.app("start")
    S.cmd({"cmd": "activate"})
    state = wait(lambda s: len(faces(s)) == 4 and (faces(s).get(frank) or "").endswith(PICTURE)
                 and PICTURE in s.get("face_pictures", []) and len(s.get("face_frames", {})) == 4, 30)
    got = faces(state)
    check("every AGENTS row has a face", len(got) == 4 and all(got.values()), json.dumps(got))
    check("Frank's face carries its agent.json picture",
          (got.get(frank) or "").startswith("face:F:") and got[frank].endswith(":" + PICTURE), str(got.get(frank)))
    check("Recruiter and Home take their card initials, with no picture",
          (got.get(recruiter) or "").startswith("face:R:") and got[recruiter].count(":") == 2
          and (got.get(home) or "").startswith("face:H:") and got[home].count(":") == 2, json.dumps(got))
    check("Content, with no card, takes its label initial", (got.get(content) or "").startswith("face:C:"), str(got.get(content)))
    check("plain rows carry no face",
          not any("face:" in r for r in state.get("spaces_rows", []) if not r.split("|")[1].startswith("agent:")))
    check("Frank's picture was fetched", PICTURE in state.get("face_pictures", []))
    tones = {row.split("|")[1][len("agent:"):]: row.split("|")[5] for row in state.get("spaces_rows", []) if row.split("|")[1].startswith("agent:")}
    check("Recruiter reads working and Home blocked", tones.get(recruiter) == "working" and tones.get(home) == "blocked", json.dumps(tones))
    for mode in ("light", "dark"):
        S.cmd({"cmd": "appearance", "mode": mode})
        state = wait(lambda s: s.get("theme", {}).get("effective") == mode)
        check(mode + " appearance applied", state.get("theme", {}).get("effective") == mode)
        time.sleep(0.5)
        state = S.state()
        png = str(ROOT / f"checks/AGENT-FACES-SPACE-{mode}.png")
        S.cmd({"cmd": "shot", "out": png})
        time.sleep(1)
        dots = {k: rgb(v) for k, v in state["face_dots"].items()}
        px = {tab: face_pixels(png, state, tab) for tab in (recruiter, frank, content, home)}
        check(mode + ": Frank draws the picture, the initials do not",
              # The black photo outweighs a tinted circle by a wide margin in either mode (about 0.25 measured).
              all(dark_share(px[frank][0]) > dark_share(px[t][0]) + 0.15 for t in (recruiter, content, home)),
              json.dumps({t: round(dark_share(px[t][0]), 2) for t in px}))
        check(mode + ": a working dot on Recruiter, a blue needs-you dot on Home",
              near(px[recruiter][1], dots["working"]) and near(px[home][1], dots["blocked"]))
        check(mode + ": idle Frank and Content show no dot",
              not any(near(px[t][1], c) for t in (frank, content) for c in dots.values()))
    S.cmd({"cmd": "appearance", "mode": "light"})

    # A card added or removed on disk reaches the row at the next poll.
    # The app reads the copy pushed into the Space, so the edit happens there.
    guest = S.guest_path(str(AGENTS))
    card = json.dumps({"name": "content", "pane": pane[content], "avatar_url": PICTURE})
    S.space("exec", f"mkdir -p {shlex.quote(guest + '/content')} && printf %s {shlex.quote(card)} > "
            f"{shlex.quote(guest + '/content/agent.json')} && rm -rf {shlex.quote(guest + '/frank')}")
    state = wait(lambda s: (faces(s).get(content) or "").endswith(PICTURE) and not (faces(s).get(frank) or "").endswith(PICTURE), 20)
    got = faces(state)
    check("an added card gives Content its picture; a removed one returns Frank to its initial",
          (got.get(content) or "").endswith(":" + PICTURE) and (got.get(frank) or "").startswith("face:F:")
          and got[frank].count(":") == 2, json.dumps(got))


if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        check("scenario completed", False, str(exc))
    finally:
        try:
            S.app("stop")
            S.lab("down")
        finally:
            pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
            pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))
