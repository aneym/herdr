#!/usr/bin/env python3
"""An agent pane opens a url and a markdown file on its tab's desk; the Shell's docs column
shows both, and both survive a live handoff of the server. Runs in the Cua Space only.

Run: HERDR_SHELL_SPACE=1 HERDR_SHELL_BIN=<herdr with desk.*> python3 scripts/check_desk_space.py
     [--out checks/DESK-SPACE.txt]
The `herdr desk` commands are typed into the lab pane (`herdr pane run`), so the CLI runs with
that pane's HERDR_PANE_ID. Against a herdr without desk.* every desk check fails (old-head proof).
Writes the report to --out and screenshots beside it named <out stem>-<step>.png.
"""
import json
import os
import pathlib
import shlex
import subprocess
import sys
import tempfile
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_desk_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = os.environ.get("DESK_SHELL_LAB", "shellspike-dk")
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.setdefault("HERDR_SPACE_OWNER", "herdr-desk-proof")
# Short enough that <lab>/h/.config/herdr/sessions/<lab name>/herdr-handoff-<pid>.sock fits sun_path.
os.environ.setdefault("SHELL_LAB_DIR", "~/.cache/herdr-build/dk")
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

OUT = ROOT / "checks/DESK-SPACE.txt"
if "--out" in sys.argv:
    OUT = pathlib.Path(sys.argv[sys.argv.index("--out") + 1]).resolve()
S.OUT = str(OUT)
lines, failures = [], []
URL = "https://example.com"
HEADING = "Desk proof heading"
BULLETS = ["first desk bullet", "second desk bullet"]


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def note(text):
    print(text, flush=True)
    lines.append("  " + text)


def lab_env():
    return dict(line.split("=", 1) for line in S.lab("env").splitlines() if "=" in line)


def lab_run(*args):
    """The lab herdr with its exit code and stderr (lab.py herdr keeps only stdout)."""
    env = lab_env()
    binary = os.path.join(env["PATH"].split(":")[0], "herdr")
    r = subprocess.run([binary, "--session", S.NAME, *args], env=env, capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.3)
    return state


def shot(step):
    path = OUT.parent / f"{OUT.stem}-{step}.png"
    S.cmd({"cmd": "shot", "out": str(path)})
    note(f"shot {path.name}")


def surface(state, pane):
    return next((s for s in state.get("surfaces", []) if s["pane"] == pane), {})


def server_desk(tab):
    snap = api("api", "snapshot")["snapshot"]
    info = next((t for t in snap["tabs"] if t["tab_id"] == tab), {})
    return info.get("desk") or {}


def server_pids():
    env = lab_env()
    binary = os.path.join(env["PATH"].split(":")[0], "herdr")
    out = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True).stdout
    return sorted(int(l.split(None, 1)[0]) for l in out.splitlines()
                  if binary in l and (f"--session {S.NAME} server" in l or "--handoff-import" in l))


def in_pane(pane, out_dir, name, command):
    """Type `command` into the lab pane with output to <out_dir>/<name>; return (rc, text)."""
    out = os.path.join(out_dir, name)
    rc = out + ".rc"
    for p in (out, rc):
        if os.path.exists(p):
            os.unlink(p)
    # The pane's cwd is out_dir, so relative names keep the typed line short.
    S.lab("herdr", "pane", "run", pane, f"{command} > {name} 2>&1; echo $? > {name}.rc")
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if os.path.exists(rc) and os.path.getsize(rc) > 0:
            return int(open(rc).read().strip() or 1), open(out).read()
        time.sleep(0.1)
    return None, open(out).read() if os.path.exists(out) else ""


def desk_view(state):
    return state.get("desk") or {}


def doc_text(state):
    return (state.get("docs") or {}).get("text", "")


def example_loaded(state):
    """example.com finished loading in the docs web view (its title, and its body text)."""
    docs = state.get("docs") or {}
    return ("example.com" in docs.get("url", "") and docs.get("title") == "Example Domain"
            and "documentation examples" in docs.get("text", ""))


def main():
    out_dir = tempfile.mkdtemp(prefix="desk-proof-")
    md = os.path.realpath(os.path.join(out_dir, "desk-proof.md"))
    with open(md, "w") as f:
        f.write(f"# {HEADING}\n\nOpened from an agent pane.\n\n" + "".join(f"- {b}\n" for b in BULLETS))
    S.app("stop")
    S.lab("down")
    S.lab("up")
    note("herdr: " + S.lab("herdr", "--version").strip() + " from " + os.environ["HERDR_SHELL_BIN"])
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "desk", "--cwd", out_dir, "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    tab, pane = made["tab"]["tab_id"], made["root_pane"]["pane_id"]
    S.app("start")
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "select", "tab": tab})
    state = wait(lambda s: s.get("selected_tab") == tab and surface(s, pane).get("cols", 0) > 0)
    check("Shell attached the agent pane on the selected tab",
          state.get("selected_tab") == tab and bool(surface(state, pane)))
    _, ready = S.wait_read(pane, lambda text: "%" in text, timeout=10)
    check("pane shell ready", ready is not None)
    check("docs column closed before any desk item", state.get("docs_visible") is False,
          f"docs_visible={state.get('docs_visible')}")
    rc, text = in_pane(pane, out_dir, "pane-id.txt", "printenv HERDR_PANE_ID")
    check("the command runs in the agent pane (HERDR_PANE_ID is the pane's)",
          rc == 0 and text.strip() == pane,
          f"HERDR_PANE_ID={text.strip()!r} pane={pane}")

    # 1. url
    rc, text = in_pane(pane, out_dir, "open-url.txt", f"herdr desk open {URL}")
    note(f"pane$ herdr desk open {URL} -> rc={rc} {text.strip()}")
    check("herdr desk open <url> succeeds from the pane", rc == 0, text.strip()[:200])
    url_id = text.split()[0] if rc == 0 and text.split() else None
    check("open prints '<item_id> <tab_id> <ref>' for this tab",
          rc == 0 and len(text.split()) >= 3 and text.split()[1] == tab and text.split()[2] == URL,
          text.strip()[:200])
    state = wait(lambda s: s.get("docs_visible") is True and desk_view(s).get("active_item") == url_id
                 and example_loaded(s), 30)
    check("the docs column opened on the tab when the url landed", state.get("docs_visible") is True,
          f"docs_visible={state.get('docs_visible')}")
    check("Shell desk dump: url item present and front/active",
          url_id is not None and [i.get("id") for i in desk_view(state).get("items", [])] == [url_id]
          and desk_view(state).get("front") == url_id and desk_view(state).get("active_item") == url_id,
          json.dumps(desk_view(state)))
    docs = state.get("docs") or {}
    check("the url item loads example.com in the docs web view",
          example_loaded(state),
          f"url={docs.get('url')!r} title={docs.get('title')!r} text={docs.get('text', '')[:80]!r}")
    shot("url")

    # 2. markdown
    rc, text = in_pane(pane, out_dir, "open-md.txt", f"herdr desk open {shlex.quote(md)}")
    note(f"pane$ herdr desk open {md} -> rc={rc} {text.strip()}")
    check("herdr desk open <md> succeeds from the pane", rc == 0, text.strip()[:200])
    md_id = text.split()[0] if rc == 0 and text.split() else None
    state = wait(lambda s: desk_view(s).get("active_item") == md_id and HEADING in doc_text(s), 30)
    items = desk_view(state).get("items", [])
    check("Shell desk dump: both items, md front and active",
          md_id is not None and [i.get("id") for i in items] == [url_id, md_id]
          and [i.get("kind") for i in items] == ["url", "file"] and items[1].get("ref") == md
          and desk_view(state).get("front") == md_id and desk_view(state).get("active_item") == md_id,
          json.dumps(desk_view(state)))
    docs = state.get("docs") or {}
    check("the docs column tabs list both desk items",
          len(docs.get("tabs", [])) >= 2 and "desk-proof.md" in docs.get("tabs", []),
          f"tabs={docs.get('tabs')}")
    check("the md item renders: heading and list text in the docs web view",
          HEADING in docs.get("text", "") and all(b in docs.get("text", "") for b in BULLETS),
          f"text={docs.get('text', '')[:120]!r}")
    check("the rendered md is html, not the raw source",
          bool(docs.get("text", "").strip()) and HEADING in docs.get("h1", [])
          and all(b in docs.get("li", []) for b in BULLETS)
          and "# " + HEADING not in docs.get("text", ""), json.dumps(docs))
    shot("md")

    # 3. herdr desk list from the pane
    rc, text = in_pane(pane, out_dir, "list.txt", "herdr desk list")
    note("pane$ herdr desk list ->")
    for line in text.splitlines():
        note("  | " + line)
    rows = [l for l in text.splitlines() if l.strip()]
    check("herdr desk list prints both items, the md one marked front (*)",
          rc == 0 and len(rows) == 2 and url_id in rows[0] and md_id in rows[1]
          and "*" in rows[1] and "*" not in rows[0], text.strip()[:200])

    # Keyboard focus must survive a new terminal key, not merely attach output.
    S.cmd({"cmd": "mouse", "pane": pane, "action": "down", "col": 2, "row": 2})
    S.cmd({"cmd": "mouse", "pane": pane, "action": "up", "col": 2, "row": 2})
    state = wait(lambda s: any(v.get("pane") == pane and v.get("first_responder") is True
                             for v in s.get("surfaces", [])), 10)
    check("the agent pane has keyboard focus before handoff",
          any(v.get("pane") == pane and v.get("first_responder") is True for v in state.get("surfaces", [])))

    # 4. live handoff of the isolated server
    before = server_desk(tab)
    pids = server_pids()
    rc, _, err = lab_run("server", "live-handoff")
    note(f"lab$ herdr server live-handoff -> rc={rc} {err.strip()[:160]}")
    check("live handoff of the lab server completes", rc == 0, err.strip()[:200])
    deadline = time.monotonic() + 15
    after_pids = server_pids()
    # The old server exits once it has handed off; wait for it to be gone.
    while time.monotonic() < deadline and (not after_pids or not set(after_pids).isdisjoint(pids)):
        time.sleep(0.3)
        after_pids = server_pids()
    check("a new server process took over", bool(pids) and bool(after_pids) and set(after_pids).isdisjoint(pids),
          f"before={pids} after={after_pids}")
    after = server_desk(tab)
    check("session.snapshot after handoff: same desk items, ids and front",
          bool(before.get("items")) and [i["id"] for i in after.get("items", [])] == [url_id, md_id]
          and [i["ref"] for i in after.get("items", [])] == [i["ref"] for i in before.get("items", [])]
          and after.get("front") == before.get("front") == md_id,
          f"before={json.dumps(before)[:160]} after={json.dumps(after)[:160]}")
    S.wait_read(pane, lambda t: "%" in t, timeout=10)
    S.lab("herdr", "pane", "run", pane, "clear")
    rc, text = in_pane(pane, out_dir, "after.txt", "echo desk-after-handoff")
    check("the agent pane survived the handoff", rc == 0 and "desk-after-handoff" in text, text[:120])
    terminal = next((p["terminal_id"] for p in api("api", "snapshot")["snapshot"]["panes"]
                     if p["pane_id"] == pane), None)

    def reattached(s):
        view = next((v for v in s.get("surfaces", []) if v.get("terminal") == terminal), {})
        return (view.get("pane") == pane and view.get("in_host") is True
                and view.get("exited") is False) and any(l.strip() == "handoff-ok-42" for l in view.get("visible_nonblank", []))

    # Arithmetic so the typed line never matches the output line, even when it wraps.
    S.lab("herdr", "pane", "run", pane, "echo handoff-ok-$((40+2))")

    state = wait(reattached, 20)
    check("the Shell re-attached the pane to its post-handoff terminal and shows new output", reattached(state),
          f"server terminal={terminal} surfaces={[(v.get('terminal'), (v.get('visible_nonblank') or [''])[-1][:40]) for v in state.get('surfaces', [])]}")
    check("the focused pane replacement is first responder after handoff",
          any(v.get("pane") == pane and v.get("terminal") == terminal and v.get("in_host") is True
              and v.get("exited") is False and v.get("first_responder") is True for v in state.get("surfaces", [])))
    state = wait(lambda s: [i.get("id") for i in desk_view(s).get("items", [])] == [url_id, md_id]
                 and desk_view(s).get("active_item") == md_id and HEADING in doc_text(s), 30)
    check("the Shell still shows both desk items after reconnect, md active",
          state.get("docs_visible") is True and [i.get("id") for i in desk_view(state).get("items", [])] == [url_id, md_id]
          and desk_view(state).get("front") == md_id and desk_view(state).get("active_item") == md_id
          and HEADING in doc_text(state),
          json.dumps(desk_view(state)) + f" docs_visible={state.get('docs_visible')} text={doc_text(state)[:60]!r}")
    shot("handoff")

    # 5. focus and close from the pane move the Shell's active item
    rc, text = in_pane(pane, out_dir, "focus.txt", f"herdr desk focus {url_id}")
    note(f"pane$ herdr desk focus {url_id} -> rc={rc} {text.strip()}")
    state = wait(lambda s: desk_view(s).get("active_item") == url_id and example_loaded(s), 30)
    check("herdr desk focus <url item> makes it the Shell's active item",
          rc == 0 and desk_view(state).get("front") == url_id and desk_view(state).get("active_item") == url_id
          and example_loaded(state), json.dumps(desk_view(state)))
    shot("focus")
    rc, text = in_pane(pane, out_dir, "close.txt", "herdr desk close")
    note(f"pane$ herdr desk close -> rc={rc} {text.strip()}")
    state = wait(lambda s: [i.get("id") for i in desk_view(s).get("items", [])] == [md_id]
                 and desk_view(s).get("active_item") == md_id and HEADING in doc_text(s), 30)
    check("herdr desk close drops the front item; the md item becomes active",
          rc == 0 and [i.get("id") for i in desk_view(state).get("items", [])] == [md_id]
          and desk_view(state).get("front") == md_id and desk_view(state).get("active_item") == md_id
          and HEADING in doc_text(state), json.dumps(desk_view(state)))
    shot("close")


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
            OUT.parent.mkdir(exist_ok=True)
            OUT.write_text("\n".join(lines) + "\n" + ("RESULT: PASS\n" if not failures
                                                      else "RESULT: FAIL " + ", ".join(failures) + "\n"))
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))
