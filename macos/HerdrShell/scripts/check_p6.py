#!/usr/bin/env python3
"""P6 check: keyboard completeness.

  SHELL_LAB=shellspike-p6 python3 scripts/check_p6.py --out checks/P6.txt

Fresh lab session -> launch app -> keys posted to the app's own pid (CGEvent through
the window server) -> verified with read-only `herdr pane read` on the lab session and
with the app's action log. Checks:

  1. dead key: option+e then e yields the single character e-acute in the pane
     (`herdr pane read`), and the accent alone leaves nothing behind.
  2. keymap: every entry in Resources/keymap.json fires its action (one
     `action <name>` line in the app log, with the pending piece named when there is
     one) and sends nothing to any pane (every lab pane reads the same before and after).
  3. terminal keys the keymap does not claim: command+backspace is turned into ctrl+u by
     the keymap's terminal list and clears the line; an unclaimed chord fires no action.
"""
import json
import os
import sys
import time
import unicodedata

os.environ.setdefault("SHELL_LAB", "shellspike-p6")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scenario as S  # noqa: E402  (helpers: lab, app, cmd, key, type_, state, pane_read, wait_read)

D = S.D
APP_LOG = os.path.join(S.LAB, "app.log")
lines, failures = [], []


def say(s=""):
    print(s)
    lines.append(s)


def check(name, ok, detail=""):
    say(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""))
    if not ok:
        failures.append(name)


def log_size():
    return os.path.getsize(APP_LOG) if os.path.exists(APP_LOG) else 0


def log_since(off):
    if not os.path.exists(APP_LOG):
        return ""
    with open(APP_LOG, "rb") as f:
        f.seek(off)
        return f.read().decode("utf-8", "replace")


def actions_since(off):
    return [l.split("action ", 1)[1] for l in log_since(off).splitlines() if "] action " in l]


def keys_sent_total():
    return sum(x["keys_sent"] for x in S.state()["surfaces"])


def api_snap():
    return S.herdr_json("api", "snapshot")["result"]["snapshot"]


def wait_for(pred, timeout=4.0):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(0.05)
    return None


def press(chord):
    mods, key = [], None
    for part in chord.split("+"):
        if part in ("cmd", "shift", "opt", "ctrl"):
            mods.append(part)
        else:
            key = part
    S.key(key, mods)


def tab_rows(st):
    """Tab ids in the order the app's goto/cycle actions use: orchestrators and lanes with their
    children, then workflows."""
    sb = st["sidebar"]
    flat = lambda rs: [x for r in rs for x in [r] + r["children"]]
    return [r["tab"] for r in flat(sb["orchestrator"]) + flat(sb["lanes"]) + sb["workflows"]]


def all_panes(snap_panes):
    return "\n=====\n".join(f"{p}:\n{S.pane_read(p)}" for p in snap_panes)


def main():
    say(f"HerdrShell P6 check  {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    keymap = json.load(open(os.path.join(D, "Resources", "keymap.json")))
    S.app("stop")
    S.lab("down")
    time.sleep(0.5)
    S.lab("up")
    snap = S.herdr_json("api", "snapshot")["result"]["snapshot"]
    tabs = {x["tab_id"]: x["label"] for x in snap["tabs"]}
    spike_tab = next(k for k, v in tabs.items() if v == "shell spike")
    lay = next(l for l in snap["layouts"] if l["tab_id"] == spike_tab)
    p1, p2 = [p["pane_id"] for p in sorted(lay["panes"], key=lambda p: p["rect"]["x"])]
    every_pane = [p["pane_id"] for p in snap["panes"]]

    # The app runs with the shipped Resources/ghostty.conf plus Alex's own Ghostty config, unmodified
    # (a copy, so the lab never reads or writes his file). Nothing here sets macos-option-as-alt:
    # dead keys have to work under the shipped default, and the shipped conf is what sets it.
    user_cfg = os.path.join(S.LAB, "user-ghostty.conf")
    home_cfg = os.path.expanduser("~/.config/ghostty/config")
    base = open(home_cfg).read() if os.path.exists(home_cfg) else ""
    with open(user_cfg, "w") as f:
        f.write(base)
    shipped = open(os.path.join(D, "Resources", "ghostty.conf")).read()
    say("config: shipped ghostty.conf sets macos-option-as-alt = "
        + next((l.split("=", 1)[1].strip() for l in shipped.splitlines() if l.startswith("macos-option-as-alt")), "NOT SET")
        + "; user config sets it: " + str("macos-option-as-alt" in base))
    check("dead-key run uses the shipped option setting (no override in the check or the user config)",
          "macos-option-as-alt = false" in shipped and "macos-option-as-alt" not in base)
    say(f"app start: {S.app('start', '--user-ghostty-config', user_cfg).strip()}")
    ready = False
    for _ in range(200):
        try:
            s = S.state()
        except SystemExit:
            time.sleep(0.1)
            continue
        surf = {x["pane"]: x for x in s["surfaces"]}
        if all(p in surf and any("%" in l for l in surf[p]["visible_nonblank"]) for p in (p1, p2)):
            ready = True
            break
        time.sleep(0.05)
    check("both scenario panes attached and rendered", ready)
    if not ready:
        return finish()
    check("pane 1 has keyboard focus on mount", s["focused_pane"] == p1, f"focused={s['focused_pane']}")
    say(f"window: key={s['window_key']} app_active={s['app_active']} post_event_access={s['post_event_access']}")
    log_text = log_since(0)
    say("keymap load line: " + next((l.split('] ', 1)[1] for l in log_text.splitlines() if "keymap:" in l), "MISSING"))

    # 1. Dead key: option+e is the acute accent, then e composes to e-acute.
    # 1a. Real system input method: it only composes for the active app, and a lab app cannot
    # be made the active app (window server refuses activation; see the state line), so this
    # run is informational: it is recorded, not scored.
    S.cmd({"cmd": "activate"})
    time.sleep(0.5)
    s = S.state()
    say(f"activation attempt: app_active={s['app_active']} window_key={s['window_key']}")
    real_input = bool(s["app_active"] and s["window_key"])
    S.key("e", ["opt"])
    time.sleep(0.15)
    S.key("e")
    time.sleep(0.3)
    real = unicodedata.normalize("NFC", S.pane_read(p1))
    real_ok = any(l.rstrip().endswith("% \u00e9") for l in real.splitlines())
    if real_input:
        check("dead key (system input method, app active): option+e then e gives e-acute", real_ok)
    else:
        say(f"[INFO] dead key through the system input method: {'e-acute' if real_ok else 'not composed'} "
            "(app inactive in the lab, so unverified here)")
    S.key("u", ["ctrl"])
    time.sleep(0.2)
    # 1b. The same keys with the input method's calls replayed through the client
    # (NSTextInputClient setMarkedText then insertText), the way the system makes them.
    S.cmd({"cmd": "ime_sim", "on": True})
    S.key("e", ["opt"])
    time.sleep(0.15)
    mid = S.pane_read(p1)
    S.key("e")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: "é" in unicodedata.normalize("NFC", x))
    S.cmd({"cmd": "ime_sim", "on": False})
    nfc = unicodedata.normalize("NFC", txt)
    prompt_lines = [l for l in nfc.splitlines() if "%" in l and "é" in l]
    check("dead key: option+e then e reached pane 1 as e-acute (herdr pane read)", dt is not None,
          f"{dt:.3f}s; line: {prompt_lines[-1].strip()!r}" if dt is not None else nfc[-300:])
    check("dead key: the pane got exactly e-acute (line is `% \u00e9`: no bare accent, no extra e)",
          bool(prompt_lines) and prompt_lines[-1].rstrip().endswith("% \u00e9"),
          f"line: {prompt_lines[-1].strip()!r}" if prompt_lines else "")
    check("dead key: the accent alone left nothing in the pane before the letter", "\u00b4" not in mid)
    say(f"herdr pane read {p1} --source recent:")
    for l in [l for l in txt.splitlines() if l.strip()][-4:]:
        say(f"  | {l}")

    # Live focus actions really move focus: focus_pane_right / left on the two-pane tab.
    S.cmd({"cmd": "select", "tab": spike_tab})
    time.sleep(0.15)
    def focused():
        return S.state()["focused_pane"]
    order = [focused()]
    S.key("l", ["cmd"]); time.sleep(0.2); order.append(focused())
    S.key("h", ["cmd"]); time.sleep(0.2); order.append(focused())
    S.key("]", ["cmd"]); time.sleep(0.2); order.append(focused())
    S.key("[", ["cmd"]); time.sleep(0.2); order.append(focused())
    check("live actions move focus: cmd+l right, cmd+h left, cmd+] next, cmd+[ previous", order == [p1, p2, p1, p2, p1], f"focus order={order}")

    # 3. Keys the keymap does not claim stay the terminal's.
    S.type_("echo abc def")
    S.key("backspace", ["cmd"])
    S.type_("echo kill-ok")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "kill-ok" for l in x.splitlines()))
    ran = [l for l in txt.splitlines() if "echo kill-ok" in l]
    check("cmd+backspace reached pane 1 as ctrl+u (line cleared; the next command ran alone)", dt is not None,
          f"{dt:.3f}s; echoed line: {ran[-1].strip()!r}" if dt is not None else txt[-300:])
    S.type_("cat -v")
    S.key("return")
    S.wait_read(p1, lambda x: "cat -v" in x)
    time.sleep(0.2)
    off = log_size()
    S.key("b", ["ctrl"])
    keys0 = keys_sent_total()
    S.key("y", ["cmd"])       # not in the keymap: no action, nothing claimed
    time.sleep(0.3)
    st = S.state()
    fs = next((x for x in st["surfaces"] if x["pane"] == st["focused_pane"]), {})
    check("an unclaimed chord (cmd+y) reached the focused pane's Ghostty surface (press delivered, physical key y + super)",
          sum(x["keys_sent"] for x in st["surfaces"]) - keys0 >= 1 and fs.get("last_key_sent") == "keycode=16 mods=8",
          f"keys_sent +{sum(x['keys_sent'] for x in st['surfaces']) - keys0}; last={fs.get('last_key_sent')!r}")
    S.key("return")
    txt, dt = S.wait_read(p1, lambda x: any(l.strip() == "^B" for l in x.splitlines()))
    check("unclaimed ctrl+b reached the pane (cat -v printed ^B)", dt is not None, f"{dt:.3f}s" if dt is not None else txt[-200:])
    check("an unclaimed chord (cmd+y) fires no keymap action", actions_since(off) == [], f"fired={actions_since(off)}")
    S.key("c", ["ctrl"])
    say(f"herdr pane read {p1} --source recent:")
    for l in [l for l in S.pane_read(p1).splitlines() if l.strip()][-8:]:
        say(f"  | {l}")
    # 2a. Every live action does its job. The app log only says an action was called; these read the
    # result (selected tab, focused pane, pane and tab counts) back from the app and from herdr.
    covered = {"next_pane", "prev_pane", "focus_pane_left", "focus_pane_right"}  # the focus check above
    st = S.state()
    rows = tab_rows(st)
    say(f"tab order: {len(rows)} tabs")
    def selected():
        return S.state()["selected_tab"]
    def goto(tab):
        S.cmd({"cmd": "select", "tab": tab})
        return wait_for(lambda: selected() == tab)
    for n in range(1, 10):
        target = rows[n - 1] if n <= len(rows) else None
        start = next((t for t in rows if t != target), spike_tab)
        goto(start)
        press(f"cmd+{n}")
        if target:
            ok = bool(wait_for(lambda: selected() == target))
            check(f"cmd+{n} (goto_tab_{n}) selects tab {n}", ok, f"selected={selected()} want={target}")
        else:
            time.sleep(0.4)
            check(f"cmd+{n} (goto_tab_{n}) with only {len(rows)} tabs changes nothing", selected() == start, f"selected={selected()} start={start}")
        covered.add(f"goto_tab_{n}")
    for chord, act, d in (("cmd+shift+]", "next_tab", 1), ("cmd+shift+[", "prev_tab", -1)):
        start = rows[1]
        goto(start)
        want = rows[(1 + d) % len(rows)]
        press(chord)
        check(f"{chord} ({act}) selects the neighbouring tab in sidebar order", bool(wait_for(lambda: selected() == want)),
              f"selected={selected()} want={want}")
        covered.add(act)
    def tab_count():
        return len(api_snap()["tabs"])
    def pane_count(tab):
        return sum(1 for p in api_snap()["panes"] if p["tab_id"] == tab)
    goto(spike_tab)
    n0 = tab_count()
    press("cmd+t")
    new_tab = wait_for(lambda: selected() if selected() != spike_tab and tab_count() == n0 + 1 else None)
    check("cmd+t (new_tab) makes one herdr tab and shows it", bool(new_tab), f"tabs {n0} -> {tab_count()}; selected={selected()}")
    covered.add("new_tab")
    goto(spike_tab)
    n0 = pane_count(spike_tab)
    press("cmd+d")
    ok = bool(wait_for(lambda: pane_count(spike_tab) == n0 + 1 and S.state()["focused_pane"] not in (p1, p2)))
    check("cmd+d (split_right) makes one herdr pane and focuses it", ok, f"panes {n0} -> {pane_count(spike_tab)}")
    covered.add("split_right")
    parent = S.state()["focused_pane"]
    press("cmd+shift+d")
    ok = bool(wait_for(lambda: pane_count(spike_tab) == n0 + 2 and S.state()["focused_pane"] not in (p1, p2, parent)))
    below = S.state()["focused_pane"]
    check("cmd+shift+d (split_down) makes one herdr pane and focuses it", ok, f"panes {n0} -> {pane_count(spike_tab)}")
    covered.add("split_down")
    press("cmd+shift+k")
    check("cmd+shift+k (focus_pane_up) moves focus to the pane above the new split", bool(wait_for(lambda: S.state()["focused_pane"] == parent)),
          f"focused={S.state()['focused_pane']} want={parent}")
    press("cmd+j")
    check("cmd+j (focus_pane_down) moves focus back to the pane below", bool(wait_for(lambda: S.state()["focused_pane"] == below)),
          f"focused={S.state()['focused_pane']} want={below}")
    covered.update({"focus_pane_up", "focus_pane_down"})
    covered.update(f"goto_space_{n}" for n in range(1, 10))   # result asserted by check_p10.py (P10)
    live = {e["action"] for e in keymap["entries"] if not e.get("pending")}
    check("every live keymap action has a result check above", live <= covered, f"unchecked={sorted(live - covered)}")

    # 2. Every keymap entry fires and sends nothing to any pane.
    say(f"keymap entries: {len(keymap['entries'])}")
    # The scenario tab is where every chord starts; tab-switching entries move away from it.
    def reset():
        S.cmd({"cmd": "select", "tab": spike_tab})
        time.sleep(0.15)

    reset()
    before_all = all_panes(every_pane)
    sent0 = keys_sent_total()
    bad = []
    for e in keymap["entries"]:
        reset()
        off = log_size()
        mods, key = [], None
        for part in e["chord"].split("+"):
            if part in ("cmd", "shift", "opt", "ctrl"):
                mods.append(part)
            else:
                key = part
        S.key(key, mods)
        fired = []
        for _ in range(40):
            fired = actions_since(off)
            if fired:
                break
            time.sleep(0.025)
        want = e["action"] + f" chord={e['chord']}" + (f" pending={e['pending']}" if e.get("pending") else "")
        ok = fired == [want]
        if not ok:
            bad.append((e["chord"], "fired=" + repr(fired)))
    reset()
    time.sleep(0.4)
    after_all = all_panes(every_pane)
    check(f"all {len(keymap['entries'])} keymap entries fire exactly their own action (app log)", not bad,
          "; ".join(f"{c}: {m}" for c, m in bad[:6]))
    check("no keymap chord reached any Ghostty surface (key presses handed to Ghostty: before == after)",
          keys_sent_total() == sent0, f"before={sent0} after={keys_sent_total()}")
    check("no keymap chord sent anything to any pane (every lab pane reads the same before and after)",
          before_all == after_all,
          "" if before_all == after_all else "\n--- before\n" + before_all[-600:] + "\n--- after\n" + after_all[-600:])
    # Actions that make panes (split, new tab) leave fresh shells behind; a chord that
    # leaked into one would show as text on its prompt line.
    now = S.herdr_json("api", "snapshot")["result"]["snapshot"]["panes"]
    fresh = [p["pane_id"] for p in now if p["pane_id"] not in every_pane]
    dirty = {}
    for p in fresh:
        extra = [l for l in S.pane_read(p).splitlines() if l.strip() and not l.rstrip().endswith("%")]
        if extra:
            dirty[p] = extra[:3]
    check(f"panes made by keymap actions ({len(fresh)}) hold only a bare prompt (nothing leaked into them)", not dirty, f"{dirty}" if dirty else "")
    kinds = {}
    for e in keymap["entries"]:
        kinds.setdefault(e.get("pending") or "live", []).append(e["chord"])
    for k, v in kinds.items():
        say(f"  {k:8} {len(v):2}  {' '.join(v)}")

    finish()


def finish():
    S.app("stop")
    time.sleep(0.5)
    left = S.sh("pgrep", "-f", f"{S.NAME}/bin/herdr terminal attach").split()
    check("attach clients exit with the app", not left, f"left={left}")
    say(f"lab down: {S.lab('down').strip()}")
    say()
    say(f"RESULT: {'PASS' if not failures else 'FAIL ' + ', '.join(failures)}")
    with open(S.OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
