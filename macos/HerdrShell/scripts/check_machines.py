#!/usr/bin/env python3
"""Other machines' chats inside the local spaces tree: a lab app against the lab server plus real machines.

  python3 scripts/check_machines.py --scratch-tab ax42/w3:t1 --scratch-pane w3:p1

Needs herdr-machine-tunnels running on this host (~/.config/herdr-machines/<name>/).
The app runs offscreen (--agent-run, --host-ok); the lab server is the only local
session it sees. It types only into the scratch pane named on the command line,
which must be a plain shell on the first machine. Writes checks/MACHINES.txt and
checks/MACHINES-live.png (the live shot is gitignored: it shows real machines).

There is no machines section (Alex, 2026-10-06): each remote agent tab is a row in the space
whose label matches its workspace's, or in a space of its own, with its machine's badge
(`@name`, or `@name:needs update` / `@name:unreachable`). A machine with no agents draws nothing.
scripts/check_machines_merge.py covers the merge rules without an app.
"""
import argparse
import json
import os
import pathlib
import subprocess
import time

os.environ["SHELL_LAB"] = "shellspike-mm"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.pop("HERDR_SHELL_SPACE", None)
ROOT = pathlib.Path(__file__).resolve().parents[1]
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/MACHINES.txt")
lines, failures = [], []


def check(name, ok, detail=""):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" ({detail})" if detail else "")
    print(line, flush=True)
    lines.append(line)
    if not ok:
        failures.append(name)


def wait(pred, timeout=30):
    deadline = time.monotonic() + timeout
    st = {}
    while time.monotonic() < deadline:
        st = S.state()
        if pred(st):
            return st
        time.sleep(0.3)
    return st


def start():
    S.app("start", "--host-ok")


def remote_read(machine, pane):
    sock = os.path.expanduser(f"~/.config/herdr-machines/{machine}")
    env = {k: v for k, v in os.environ.items() if not k.startswith("HERDR_")}
    env.update(HERDR_SOCKET_PATH=sock + "/herdr.sock", HERDR_CLIENT_SOCKET_PATH=sock + "/herdr-client.sock")
    r = subprocess.run([os.path.expanduser("~/.local/bin/herdr"), "pane", "read", pane, "--source", "recent",
                        "--lines", "20"], env=env, capture_output=True, text=True)
    return r.stdout


def remote_snapshot(machine):
    sock = os.path.expanduser(f"~/.config/herdr-machines/{machine}")
    env = {k: v for k, v in os.environ.items() if not k.startswith("HERDR_")}
    env.update(HERDR_SOCKET_PATH=sock + "/herdr.sock", HERDR_CLIENT_SOCKET_PATH=sock + "/herdr-client.sock")
    r = subprocess.run([os.path.expanduser("~/.local/bin/herdr"), "api", "snapshot"], env=env,
                       capture_output=True, text=True)
    return json.loads(r.stdout)["result"]["snapshot"]


def remote_rows(rows):
    """Rows another machine adds: its chats (they name a remote tab) and spaces of its own."""
    return [r for r in rows if "/" in r.split("|")[1]]


def local_part(rows):
    """Rows the local session draws. Footer rows read the live host overlay, which moves between
    runs, so only their ids count; the pinned header's count includes remote pins."""
    own = {r.split("|")[1][len("space:"):] for r in rows if r.startswith("space|space:") and "/" in r.split("|")[1]}
    keep = [r for r in rows if r not in remote_rows(rows) and not r.startswith("section|pinned|")
            and not any(r.split("|")[1].startswith(f"section:{o}:") or r.split("|")[1].startswith(f"group:{o}:") for o in own)]
    return [r.split("|")[1] if r.startswith(("footerUsage|", "footerHost|")) else r for r in keep]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--machines", default="ax42,pc,forge")
    ap.add_argument("--scratch-tab", required=True)
    ap.add_argument("--scratch-pane", required=True)
    a = ap.parse_args()
    names = a.machines.split(",")
    machine = a.scratch_tab.split("/")[0]
    lab_dir = pathlib.Path(S.lab("env").split("HOME=", 1)[1].splitlines()[0]).parent
    lab_dir.mkdir(parents=True, exist_ok=True)
    # app.py and run.sh keep the FIFO and app.log under the unhashed name.
    app_dir = pathlib.Path.home() / ".cache/herdr-build" / os.environ["SHELL_LAB"]
    (app_dir / "bin").mkdir(parents=True, exist_ok=True)
    cfg = lab_dir / "machines.json"
    cfg.write_text(json.dumps({"machines": [{"name": n, "dir": f"~/.config/herdr-machines/{n}"} for n in names]
                              + [{"name": "ghost", "dir": str(lab_dir / "no-such-machine")}]}))
    subprocess.run(["defaults", "delete", "herdr.shell.dev.shellspike-mm"], capture_output=True)
    S.app("stop")
    S.lab("down")
    S.lab("up")
    link = app_dir / "bin/herdr"
    if not link.exists():
        link.symlink_to(lab_dir / "bin/herdr")
    os.environ.pop("HERDR_SHELL_MACHINES", None)
    start()
    base = wait(lambda s: len(s.get("spaces_rows", [])) >= 3)
    base_rows = base.get("spaces_rows", [])
    check("baseline has no remote rows", not remote_rows(base_rows))
    S.app("stop")

    os.environ["HERDR_SHELL_MACHINES"] = str(cfg)
    start()
    snaps = {n: remote_snapshot(n) for n in names}
    with_agents = [n for n in names if snaps[n]["agents"]]
    st = wait(lambda s: all(any(r.endswith(f"|@{n}") or f"|@{n}:" in r for r in s.get("spaces_rows", []))
                            for n in with_agents), timeout=40)
    rows = st.get("spaces_rows", [])
    (lab_dir / "rows.txt").write_text("\n".join(rows))
    diff = [(x, y) for x, y in zip(local_part(rows), local_part(base_rows)) if x != y]
    check("local rows unchanged with machines", local_part(rows) == local_part(base_rows),
          f"{len(local_part(rows))} vs {len(local_part(base_rows))}; first diff {diff[:1]}")
    check("no machines section", not any(r.startswith(("machine|", "title|machines|")) for r in rows))
    local_snap = json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"]
    local_protocol = local_snap["protocol"]
    local_labels = {(w.get("label") or "").strip().lower(): w["workspace_id"] for w in local_snap["workspaces"]}
    for n in names:
        snap = snaps[n]
        mismatch = snap.get("protocol") is not None and snap.get("protocol") != local_protocol
        badge = f"@{n}:needs update" if mismatch else f"@{n}"
        # A pinned tab is a chat row too, agent or not.
        agent_tabs = {a["tab_id"] for a in snap["agents"]} | {t["tab_id"] for t in snap["tabs"] if t.get("pin_index") is not None}
        drawn = [r for r in rows if r.startswith(f"tab|tab:{n}/")]
        check(f"{n} draws one row per agent tab", sorted(r.split("|")[1] for r in drawn)
              == sorted(f"tab:{n}/{t}" for t in agent_tabs), f"{len(drawn)} rows, {len(agent_tabs)} agent tabs")
        check(f"{n} rows carry the badge, needs update only on a protocol mismatch",
              all(r.split("|")[-1] == badge for r in drawn), f"remote {snap.get('protocol')} local {local_protocol}")
        homes = []
        for t in snap["tabs"]:
            if t["tab_id"] not in agent_tabs:
                continue
            ws = next(w for w in snap["workspaces"] if w["workspace_id"] == t["workspace_id"])
            home = local_labels.get((ws.get("label") or "").strip().lower())
            space = f"space:{home}" if home else f"space:{n}/{ws['workspace_id']}"
            i = next((k for k, r in enumerate(rows) if r.split("|")[1] == space), -1)
            j = next((k for k, r in enumerate(rows) if r.split("|")[1] == f"tab:{n}/{t['tab_id']}"), -1)
            nxt = next((k for k in range(i + 1, len(rows)) if rows[k].split("|")[0] in ("space", "title", "hidden")), len(rows))
            homes.append(i >= 0 and i < j < nxt)
        check(f"{n} chats sit in the space their label names", all(homes), f"{homes.count(False)} misplaced")
    check("a machine with no agents draws nothing",
          all(not any(f"{n}/" in r for r in rows) for n in names if n not in with_agents))
    hosts = [r.split("|")[6] for r in rows if r.startswith("footerHost|")]
    clash = [h for h in hosts if any(h.lower() == n.lower() and h != n for n in names)]
    check("host footer uses the machine names", not clash, f"footer {hosts}")
    check("unreachable machine adds no row", not any("ghost" in r for r in rows))

    S.cmd({"cmd": "select", "tab": a.scratch_tab})
    st = wait(lambda s: s.get("selected_tab") == a.scratch_tab and any(
        x.get("pane") == f"{machine}/{a.scratch_pane}" and x.get("visible_nonblank") for x in s.get("surfaces", [])))
    surf = next((x for x in st.get("surfaces", []) if x.get("pane") == f"{machine}/{a.scratch_pane}"), {})
    check("remote tab selected and attached", bool(surf.get("visible_nonblank")), json.dumps(surf.get("visible_nonblank", [])[-2:]))
    # Keys go to the focused pane. Type only when the scratch tab is that one shell and it has focus.
    shown = (st.get("shown_layout") or {}).get("panes", [])
    ready = (len(shown) == 1 and shown[0].get("pane") == f"{machine}/{a.scratch_pane}"
             and st.get("focused_pane") == f"{machine}/{a.scratch_pane}" and surf.get("visible_nonblank"))
    check("scratch tab is one focused shell", bool(ready), json.dumps({"shown": shown, "focused": st.get("focused_pane")}))
    if not ready:
        S.app("stop")
        S.lab("down")
        pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
        raise SystemExit("not typing: the scratch pane is not the only, focused pane of its tab")
    token = f"MM_SHELL_{int(time.time()) % 100000}"
    S.type_(f"echo {token}")
    S.key("return")
    deadline = time.monotonic() + 10
    text = ""
    while time.monotonic() < deadline:
        text = remote_read(machine, a.scratch_pane)
        if token in text.replace(f"echo {token}", ""):
            break
        time.sleep(0.3)
    check("typing reaches the remote pane", token in text.replace(f"echo {token}", ""), text.strip().splitlines()[-1:] and text.strip().splitlines()[-1])
    S.cmd({"cmd": "shot", "out": str(ROOT / "checks/MACHINES-live.png")})

    # Quit on a remote tab, drop that machine from the config: the relaunch must fall back to a local tab.
    S.cmd({"cmd": "select", "tab": a.scratch_tab})
    wait(lambda s: s.get("selected_tab") == a.scratch_tab, timeout=10)
    S.app("stop")
    cfg.write_text(json.dumps({"machines": [{"name": n, "dir": f"~/.config/herdr-machines/{n}"} for n in names if n != machine]}))
    start()
    st = wait(lambda s: (s.get("selected_tab") or "").startswith(("w", "s")) and "/" not in (s.get("selected_tab") or "/"), timeout=20)
    check("removed machine's tab falls back to a local tab", "/" not in (st.get("selected_tab") or "/"), str(st.get("selected_tab")))
    S.app("stop")
    S.lab("down")
    pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    print(f"{len(lines) - len(failures)}/{len(lines)} passed")
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()
