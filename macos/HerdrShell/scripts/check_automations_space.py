#!/usr/bin/env python3
import json
import os
import pathlib
import shutil
import sys
import time

if os.environ.get("HERDR_SHELL_SPACE") != "1":
    raise SystemExit("check_automations_space requires HERDR_SHELL_SPACE=1; host launch is forbidden")
os.environ["SHELL_LAB"] = "shellspike-autos"
os.environ.setdefault("HERDR_SHELL_BIN", os.path.expanduser("~/.local/bin/herdr"))
os.environ.setdefault("HERDR_SPACE_OWNER", "check-automations")
ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import scenario as S  # noqa: E402

S.OUT = str(ROOT / "checks/AUTOMATIONS-SPACE.txt")
SHOTS = ROOT / "checks"
EVIDENCE = pathlib.Path.home() / ".agent-rails/factory/automations/evidence"
URL = "https://studio.tailf266ac.ts.net:8950/plans/plan-13a9879314e8469d"
FIXTURES = {
    "pinned-watchdog": {
        "id": "pinned-watchdog", "name": "Pinned tab watchdog", "cadence": "every 20 min", "status": "ok",
        "result": "14 tabs: 7 working, 4 idle-done, 2 thrashing, 1 waiting-on-alex",
        "last_run": "2026-10-24T18:21:01Z", "next_run": "2026-10-24T18:40:00Z",
        "report": "/tmp/pinned-watchdog/20261024T182101Z.md", "report_url": URL,
    },
    "zz-nightly-sweep": {
        "id": "nightly-sweep", "name": "Nightly sweep", "cadence": "every 1 hour", "status": "error",
        "result": "failed: route pools timed out", "last_run": "2026-10-24T18:00:00Z",
    },
}
lines, failures = [], []


def check(name, condition, detail=""):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}" + (f" ({detail})" if detail and not condition else "")
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def api(*args):
    return json.loads(S.lab("herdr", *args))["result"]


def wait(predicate, timeout=10):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(0.3)
    return state


def rows(state):
    return [r.split("|") for r in state.get("spaces_rows", [])]


def block(state):
    out, inside = [], False
    for row in rows(state):
        if row[1] == "automations":
            inside = True
        elif inside and row[0] != "automation":
            break
        if inside:
            out.append(row)
    return out


def write_fixtures():
    env = dict(line.split("=", 1) for line in S.lab("env").splitlines())
    root = pathlib.Path(env["HOME"]) / ".agent-rails/factory/automations"
    for folder, record in FIXTURES.items():
        (root / folder).mkdir(parents=True, exist_ok=True)
        (root / folder / "latest.json").write_text(json.dumps(record))


def shoot(mode):
    S.cmd({"cmd": "appearance", "mode": mode})
    wait(lambda s: s.get("theme", {}).get("effective") == mode)
    time.sleep(0.8)
    png = SHOTS / f"AUTOMATIONS-SPACE-{mode}.png"
    png.parent.mkdir(parents=True, exist_ok=True)
    S.cmd({"cmd": "shot", "out": str(png), "scale": 2})
    time.sleep(1.5)
    check(f"{mode} Shell window shot written", png.exists() and png.stat().st_size > 0)
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    shutil.copy2(png, EVIDENCE / png.name)


def main():
    S.app("stop")
    S.lab("down")
    S.lab("up")
    old = api("workspace", "list")["workspaces"]
    made = api("workspace", "create", "--label", "factory", "--no-focus")
    for workspace in old:
        S.lab("herdr", "workspace", "close", workspace["workspace_id"])
    ws, first = made["workspace"]["workspace_id"], made["tab"]["tab_id"]
    S.lab("herdr", "tab", "rename", first, "factory lead")
    second = api("tab", "create", "--workspace", ws, "--label", "rails build", "--no-focus")["tab"]["tab_id"]
    S.lab("herdr", "tab", "pin", first)
    S.lab("herdr", "tab", "pin", second)
    write_fixtures()
    deadline = time.monotonic() + 40
    served = []
    while time.monotonic() < deadline and len(served) < 2:
        served = json.loads(S.lab("herdr", "api", "snapshot"))["result"]["snapshot"].get("automations", [])
        time.sleep(1)
    check("server serves both fixture automations in session.snapshot",
          [a["id"] for a in served] == ["pinned-watchdog", "nightly-sweep"], json.dumps(served)[:300])

    S.app("start")
    S.cmd({"cmd": "activate"})
    state = wait(lambda s: len(block(s)) == 5, 30)
    got = block(state)
    ids = [r[1] for r in rows(state)]
    check("AUTOMATIONS header follows the PINNED block directly",
          "automations" in ids and ids.index("automations") == ids.index("pinned:" + second) + 1, json.dumps(ids))
    check("header, two rows and one detail row per result",
          [r[1] for r in got] == ["automations", "automation:pinned-watchdog", "automationresult:pinned-watchdog",
                                  "automation:nightly-sweep", "automationresult:nightly-sweep"], json.dumps(got))
    if len(got) == 5:
        head, ok, ok_detail, bad, bad_detail = got
        check("header reads AUTOMATIONS like PINNED", head[0] == "section" and head[6] == "AUTOMATIONS", json.dumps(head))
        check("ok row: done glyph, name, cadence and next run",
              ok[4:7] == ["■", "done", "Pinned tab watchdog"] and ok[7].startswith("20m · next "), json.dumps(ok))
        check("detail row is dim, one level in, with the result",
              ok_detail[2] == "1" and ok_detail[12] == "dim" and ok_detail[6] == FIXTURES["pinned-watchdog"]["result"],
              json.dumps(ok_detail))
        check("error row: bad tone, cadence without a next run", bad[5] == "blocked" and bad[7] == "1h", json.dumps(bad))
        check("error detail shows the failure", bad_detail[6] == "failed: route pools timed out", json.dumps(bad_detail))

    shoot("light")
    shoot("dark")

    before = state.get("selected_tab")
    opened = len(state.get("opened_urls", []))
    S.cmd({"cmd": "activate"})
    S.cmd({"cmd": "click", "target": "spaces_row", "row": "automation:pinned-watchdog"})
    state = wait(lambda s: len(s.get("opened_urls", [])) > opened, 6)
    urls = state.get("opened_urls", [])
    check("clicking the row opens its report in Aside", urls[-1:] == ["aside " + URL], json.dumps(urls[-2:]))
    check("the click does not change the selected tab", state.get("selected_tab") == before,
          json.dumps([before, state.get("selected_tab")]))
    opened = len(urls)
    S.cmd({"cmd": "click", "target": "spaces_row", "row": "automation:nightly-sweep"})
    time.sleep(1.5)
    state = S.state()
    check("a row without report_url opens nothing", len(state.get("opened_urls", [])) == opened,
          json.dumps(state.get("opened_urls", [])[-2:]))


if __name__ == "__main__":
    try:
        main()
    except (Exception, SystemExit) as exc:
        check("scenario completed", False, repr(exc)[:400])
    finally:
        try:
            S.app("stop")
            S.lab("down")
        finally:
            lines.append(f"{sum(l.startswith('[PASS]') for l in lines)} passed; {len(failures)} failed")
            pathlib.Path(S.OUT).parent.mkdir(exist_ok=True)
            pathlib.Path(S.OUT).write_text("\n".join(lines) + "\n")
    if failures:
        raise SystemExit("FAIL: " + ", ".join(failures))
