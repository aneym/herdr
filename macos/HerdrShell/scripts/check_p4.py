#!/usr/bin/env python3
"""P4 check: kind and host from agent tokens, with label fallback.

  SHELL_LAB=shellspike-p4 python3 scripts/check_p4.py --out checks/P4.txt

Lab (seeded by lab.py, plus tabs added here): one token-tagged workflow (kind=workflow,
host=forge-1, owned by a lane, label without "wf ") and the untagged `wf recruiter-2320`
(no kind token, host=PC, owned by the recruiter lane). Both must fold under the right
lane with the right host badge. Extras: a tagged workflow with no owner lands in WORKFLOWS;
a tab labelled "wf ..." but tagged kind=lane stays a lane; an unknown kind value falls back.
"""
import json
import os
import subprocess
import sys
import time

os.environ.setdefault("SHELL_LAB", "shellspike-p4")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402  (helpers: lab, app, cmd, state, herdr_json)

lines, failures = [], []


def say(s=""):
    print(s)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def herdr(*a):
    return S.lab("herdr", *a)


def create_tab(ws, label):
    r = json.loads(herdr("tab", "create", "--workspace", ws, "--label", label, "--cwd", "/tmp", "--no-focus"))["result"]
    return r["root_pane"]["pane_id"]


def report(pane, agent, state, tokens):
    for _ in range(100):
        if "%" in herdr("pane", "read", pane, "--source", "visible"):
            break
        time.sleep(0.05)
    herdr("pane", "report-agent", pane, "--source", "spike", "--agent", agent, "--state", state)
    if tokens:
        args = ["pane", "report-metadata", pane, "--source", "spike"]
        for k, v in tokens.items():
            args += ["--token", f"{k}={v}"]
        herdr(*args)


def find(rows, label):
    for r in rows:
        if r["label"] == label:
            return r
    return None


def main():
    say(f"HerdrShell P4 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    ws = snap["workspaces"][0]["workspace_id"]
    tab_pane = {}
    for t in snap["tabs"]:
        tab_pane[t["label"]] = next(p["pane_id"] for p in snap["panes"] if p["tab_id"] == t["tab_id"])

    emb = create_tab(ws, "embeddings")                 # untagged lane (fallback)
    tagged_wf = create_tab(ws, "embed run 7")          # tagged workflow, label has no "wf "
    tagged_free = create_tab(ws, "nightly sweep")      # tagged workflow, no owner
    lane_wf = create_tab(ws, "wf misnamed")            # "wf" label but kind=lane
    bad_kind = create_tab(ws, "odd kind")              # unknown kind value -> fallback (lane)
    report(emb, "claude", "idle", {})
    report(tagged_wf, "codex", "working", {"kind": "workflow", "host": "forge-1"})
    report(tagged_free, "codex", "idle", {"kind": "workflow", "host": "forge-1"})
    report(lane_wf, "claude", "idle", {"kind": "lane"})
    report(bad_kind, "claude", "idle", {"kind": "banana"})
    herdr("agent", "owner", "set", tagged_wf, emb)

    say(f"app start: {S.app('start').strip()}")
    side = None
    for _ in range(200):
        try:
            s = S.state()
        except SystemExit:
            time.sleep(0.1)
            continue
        side = s["sidebar"]
        if find(side["lanes"], "embeddings") and find(side["workflows"], "nightly sweep"):
            break
        time.sleep(0.05)
    if side is None:
        check("app state readable", False)
        return finish()

    def kids(row):
        return [(c["label"], c["host"]) for c in row["children"]] if row else None

    rec = find(side["lanes"], "recruiter")
    check("untagged `wf recruiter-2320` (owner set, host token PC) folds under lane 'recruiter' with badge PC",
          kids(rec) == [("wf recruiter-2320", "PC")], f"children={kids(rec)}")
    emb_row = find(side["lanes"], "embeddings")
    check("token-tagged workflow 'embed run 7' (kind=workflow, host=forge-1) folds under lane 'embeddings' with badge forge-1",
          kids(emb_row) == [("embed run 7", "forge-1")], f"children={kids(emb_row)}")
    orch = find(side["orchestrator"], "rails orchestrator")
    check("untagged `wf embed wave-a` folds under the orchestrator, badge PC",
          kids(orch) == [("wf embed wave-a", "PC")], f"children={kids(orch)}")
    free = find(side["workflows"], "nightly sweep")
    check("tagged workflow with no owner sits in WORKFLOWS with badge forge-1",
          free is not None and free["host"] == "forge-1" and free["kind"] == "workflow", f"row={free and (free['label'], free['host'])}")
    lw = find(side["lanes"], "wf misnamed")
    check("kind=lane token beats a 'wf ' label", lw is not None and lw["kind"] == "lane")
    bk = find(side["lanes"], "odd kind")
    check("unknown kind value falls back to label rules (lane)", bk is not None and bk["kind"] == "lane")
    hosts = {h["host"]: h["tabs"] for h in s["hosts"]}
    check("hosts row counts follow host tokens", hosts.get("forge-1") == 2 and hosts.get("PC") == 2, f"hosts={hosts}")
    say("sidebar sections:")
    for sec in ("orchestrator", "lanes", "workflows"):
        for r in side[sec]:
            say(f"  {sec.upper():12} {r['label']:22} kind={r['kind']:12} {r['host']:8} folded: {kids(r)}")
    finish()


def finish():
    S.check_front(check)
    S.app("stop")
    time.sleep(0.5)
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
