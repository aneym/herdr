#!/usr/bin/env python3
"""P12 check: hosts row.

  SHELL_LAB=shellspike12 python3 scripts/check_p12.py --out checks/P12.txt

Lab: the seeded tabs (two host=PC workflows) plus new tabs tagged host=forge-1 (two) and
host=PC (one more). Run 1, default provider (herdr-only): the hosts row's per-host tab
counts match counts computed independently from `herdr api snapshot` agent tokens, and no
slot or session numbers appear. Run 2, stub provider (`--hosts-stub FILE`): the stub's slot
and session numbers appear in the hosts row, the tab counts still match the tokens, a host
only the provider knows shows with 0 tabs, and editing the stub file while the app runs
updates the row.
"""
import json
import os
import sys
import tempfile
import time

os.environ.setdefault("SHELL_LAB", "shellspike12")
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


def expected_counts():
    """Tabs per host from herdr's own snapshot: first non-empty `host` token across the
    tab's agents, else Studio. Independent of the app's code."""
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    out = {}
    for t in snap["tabs"]:
        host = "Studio"
        for a in snap["agents"]:
            v = (a.get("tokens") or {}).get("host", "").strip()
            if a["tab_id"] == t["tab_id"] and v:
                host = v
                break
        out[host] = out.get(host, 0) + 1
    return out


def wait_hosts(pred, timeout=8.0):
    end = time.time() + timeout
    s = None
    while time.time() < end:
        try:
            s = S.state()
        except SystemExit:
            time.sleep(0.1)
            continue
        if pred(s):
            return s
        time.sleep(0.1)
    return s


def by_host(s):
    return {h["host"]: h for h in s["hosts"]}


def main():
    say(f"HerdrShell P12 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    ws = snap["workspaces"][0]["workspace_id"]
    pcs = create_tab(ws, "pc job")
    f1 = create_tab(ws, "forge job a")
    f2 = create_tab(ws, "forge job b")
    report(pcs, "codex", "working", {"host": "PC"})
    report(f1, "codex", "working", {"host": "forge-1"})
    report(f2, "codex", "idle", {"host": "forge-1"})
    exp = expected_counts()
    say(f"expected tabs per host from herdr snapshot tokens: {json.dumps(exp, sort_keys=True)}")
    check("lab has host=PC and host=forge-1 tabs", exp.get("PC") == 3 and exp.get("forge-1") == 2, f"expected={exp}")

    # Run 1: default provider.
    say(f"app start (default provider): {S.app('start').strip()}")
    s = wait_hosts(lambda s: {h: v["tabs"] for h, v in by_host(s).items()} == exp)
    if s is None:
        check("app state readable", False)
        return finish()
    hosts = by_host(s)
    say("hosts row (default): " + "; ".join(h["text"] for h in s["hosts"]))
    check("default provider is herdr-only", s.get("hosts_provider") == "herdr-only", f"provider={s.get('hosts_provider')}")
    check("hosts row tab counts match host tokens (PC and forge-1)",
          {h: v["tabs"] for h, v in hosts.items()} == exp, f"row={ {h: v['tabs'] for h, v in hosts.items()} } expected={exp}")
    check("no slot or session numbers without a provider",
          all(v["slots_used"] is None and v["slots_total"] is None and v["sessions"] is None for v in hosts.values())
          and not any("slots" in v["text"] or "session" in v["text"] for v in hosts.values()))
    S.app("stop")
    time.sleep(0.7)

    # Run 2: stubbed provider.
    stub = os.path.join(tempfile.mkdtemp(prefix="p12-"), "hosts.json")
    with open(stub, "w") as f:
        json.dump({"forge-1": {"slots_used": 3, "slots_total": 8, "sessions": 5},
                   "PC": {"slots_used": 1, "slots_total": 4, "sessions": 1},
                   "box-9": {"slots_used": 0, "slots_total": 6, "sessions": 0}}, f)
    say(f"app start (stub provider {stub}): {S.app('start', '--hosts-stub', stub).strip()}")
    s = wait_hosts(lambda s: s.get("hosts_provider") == "file-stub" and by_host(s).get("forge-1", {}).get("slots_total") == 8)
    hosts = by_host(s)
    say("hosts row (stub): " + "; ".join(h["text"] for h in s["hosts"]))
    check("stub provider active", s.get("hosts_provider") == "file-stub")
    f1r, pcr = hosts.get("forge-1", {}), hosts.get("PC", {})
    check("stub slot numbers appear for forge-1 (3/8, 5 sessions) and PC (1/4, 1 session)",
          (f1r.get("slots_used"), f1r.get("slots_total"), f1r.get("sessions")) == (3, 8, 5)
          and (pcr.get("slots_used"), pcr.get("slots_total"), pcr.get("sessions")) == (1, 4, 1),
          f"forge-1={f1r.get('text')!r} PC={pcr.get('text')!r}")
    check("rendered text carries the slots", "slots 3/8" in f1r.get("text", "") and "slots 1/4" in pcr.get("text", ""))
    check("tab counts still follow host tokens under the stub",
          all(hosts.get(h, {}).get("tabs") == n for h, n in exp.items()), f"row={ {h: v['tabs'] for h, v in hosts.items()} }")
    b9 = hosts.get("box-9", {})
    check("a host only the provider knows shows with 0 tabs and its slots",
          b9.get("tabs") == 0 and b9.get("slots_total") == 6, f"box-9={b9.get('text')!r}")
    check("Studio (no stub entry) shows tab count only",
          "slots" not in hosts.get("Studio", {}).get("text", "x") and hosts.get("Studio", {}).get("tabs") == exp.get("Studio"),
          f"Studio={hosts.get('Studio', {}).get('text')!r}")
    S.cmd({"cmd": "shot", "out": os.path.splitext(S.OUT)[0] + ".png"})

    with open(stub, "w") as f:
        json.dump({"forge-1": {"slots_used": 7, "slots_total": 8, "sessions": 9}}, f)
    os.utime(stub, (time.time() + 2, time.time() + 2))
    s = wait_hosts(lambda s: by_host(s).get("forge-1", {}).get("slots_used") == 7)
    f1r = by_host(s).get("forge-1", {})
    check("editing the stub while the app runs updates the row (forge-1 7/8, 9 sessions; PC and box-9 stats gone)",
          f1r.get("slots_used") == 7 and f1r.get("sessions") == 9 and by_host(s).get("PC", {}).get("slots_total") is None
          and "box-9" not in by_host(s), f"forge-1={f1r.get('text')!r}")
    finish()


def finish():
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
