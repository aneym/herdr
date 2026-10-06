#!/usr/bin/env python3
"""Machines beside the local session: a lab app against the lab server plus real machines.

  python3 scripts/check_machines.py --scratch-tab ax42/w3:t1 --scratch-pane w3:p1

Needs herdr-machine-tunnels running on this host (~/.config/herdr-machines/<name>/).
The app runs offscreen (--agent-run, --host-ok); the lab server is the only local
session it sees. It types only into the scratch pane named on the command line,
which must be a plain shell on the first machine. Writes checks/MACHINES.txt and
checks/MACHINES-live.png (the live shot is gitignored: it shows real machines).
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


def expected_sections(snap):
    """Workspace labels the machine block should draw: one over agent tabs, or over a shells row
    that another workspace's shells row sits beside. A lone shells row gets no label."""
    agent_tabs = {a["tab_id"] for a in snap["agents"]}
    filled = [w for w in snap["workspaces"] if any(t["workspace_id"] == w["workspace_id"] for t in snap["tabs"])]
    return sorted(w["workspace_id"] for w in filled if len(filled) > 1 or any(
        t["workspace_id"] == w["workspace_id"] and t["tab_id"] in agent_tabs for t in snap["tabs"]))


def local_part(rows):
    """Rows the local session draws: everything outside the machines block. Footer rows read the
    live host overlay, which moves between runs, so only their ids count."""
    if "title|machines|" in "\n".join(rows):
        i = next(i for i, r in enumerate(rows) if r.startswith("title|machines|"))
        j = next((k for k in range(i, len(rows)) if rows[k].startswith(("footerUsage|", "footerHost|"))), len(rows))
        rows = rows[:i] + rows[j:]
    return [r.split("|")[1] if r.startswith(("footerUsage|", "footerHost|")) else r for r in rows]


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
    check("baseline has no machine rows", not any(r.startswith(("machine|", "title|machines|")) for r in base_rows))
    S.app("stop")

    os.environ["HERDR_SHELL_MACHINES"] = str(cfg)
    start()
    st = wait(lambda s: all(any(r.startswith(f"machine|machine:{n}|") and "|connecting|" not in r
                                for r in s.get("spaces_rows", [])) for n in names), timeout=40)
    rows = st.get("spaces_rows", [])
    (lab_dir / "rows.txt").write_text("\n".join(rows))
    diff = [(x, y) for x, y in zip(local_part(rows), local_part(base_rows)) if x != y]
    check("local rows unchanged with machines", local_part(rows) == local_part(base_rows),
          f"{len(local_part(rows))} vs {len(base_rows)}; first diff {diff[:1]}")
    check("machines block after local spaces", "title|machines|" in "\n".join(rows))
    local_protocol = json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"]["protocol"]
    for n in names:
        row = next((r for r in rows if r.startswith(f"machine|machine:{n}|")), "")
        snap = remote_snapshot(n)
        check(f"{n} header names no version", "|herdr " not in row, row)
        mismatch = snap.get("protocol") != local_protocol
        check(f"{n} says needs update only on a protocol mismatch",
              ("|needs update|" in row) == mismatch, f"remote {snap.get('protocol')} local {local_protocol}: {row}")
        sections = sorted(r.split("|")[1][len(f"msection:{n}/"):] for r in rows if r.startswith(f"section|msection:{n}/"))
        check(f"{n} labels only workspaces with lanes", sections == expected_sections(snap),
              f"drawn {sections} expected {expected_sections(snap)}")
    hosts = [r.split("|")[6] for r in rows if r.startswith("footerHost|")]
    clash = [h for h in hosts if any(h.lower() == n.lower() and h != n for n in names)]
    check("host footer uses the machine names", not clash, f"footer {hosts}")
    ghost = next((r for r in rows if r.startswith("machine|machine:ghost|")), "")
    check("unreachable machine says so", "|connecting|" in ghost or "|offline" in ghost, ghost)
    agent_tabs = [r for r in rows if r.startswith(f"tab|tab:{machine}/")]
    check(f"{machine} agent tabs listed", len(agent_tabs) > 0, f"{len(agent_tabs)} rows")

    S.cmd({"cmd": "spaces_click", "row": f"group:{machine}/{a.scratch_tab.split('/')[1].split(':')[0]}:shells"})
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

    S.cmd({"cmd": "spaces_click", "row": f"machine:{machine}"})
    st = wait(lambda s: any(r.startswith(f"machine|machine:{machine}|") and "|closed|" in r for r in s.get("spaces_rows", [])))
    row = next((r for r in st.get("spaces_rows", []) if r.startswith(f"machine|machine:{machine}|")), "")
    check("machine folds to one counted line", "|closed|" in row and " agent" in row, row)
    check("folded machine hides its tabs", not any(r.startswith(f"tab|tab:{machine}/") for r in st.get("spaces_rows", [])))
    S.cmd({"cmd": "spaces_click", "row": f"machine:{machine}"})
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
