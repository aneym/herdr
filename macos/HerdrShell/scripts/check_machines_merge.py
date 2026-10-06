#!/usr/bin/env python3
"""Other machines' chats join their spaces with a badge; no machines section (Alex, 2026-10-06).

  python3 scripts/check_machines_merge.py

Pure: compiles SpacesTree.swift and MachineMerge.swift with a dump driver and reads the rows
for scripts/fixtures/machines/merge.json, with and without the machines. No app, no server.
Writes checks/MACHINES-MERGE.txt.
"""
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "scripts/fixtures/machines/merge.json"
BUILD = pathlib.Path.home() / ".cache/herdr-build/machines-merge"
BUILD.mkdir(parents=True, exist_ok=True)
DRIVER = BUILD / "machines_merge_dump"
subprocess.run(["swiftc", str(ROOT / "Sources/HerdrShell/SpacesTree.swift"), str(ROOT / "Sources/HerdrShell/MachineMerge.swift"),
                str(ROOT / "scripts/machines_merge_dump.swift"), "-o", str(DRIVER)], check=True)
rows = subprocess.check_output([str(DRIVER), str(FIXTURE)], text=True).splitlines()
local = subprocess.check_output([str(DRIVER), str(FIXTURE), "--local"], text=True).splitlines()
failures, lines = [], []


def check(name, ok, detail=""):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" ({detail})" if detail and not ok else "")
    print(line)
    lines.append(line)
    if not ok:
        failures.append(name)


def fields(row_id):
    return next((r.split("|") for r in rows if r.split("|")[1] == row_id), None)


def at(row_id):
    return next((i for i, r in enumerate(rows) if r.split("|")[1] == row_id), -1)


def block(space_id):
    """Row ids from a space row up to the next space, group title or hidden row."""
    start = at("space:" + space_id)
    end = next((i for i in range(start + 1, len(rows)) if rows[i].split("|")[0] in ("space", "title", "hidden", "footerHost", "footerUsage")), len(rows))
    return [r.split("|")[1] for r in rows[start:end]] if start >= 0 else []


check("no machines section", not any(r.startswith("title|machines|") or r.startswith("machine|") for r in rows))
rails = block("s1")
check("remote chat sits inside the space its label matches", "tab:ax42/w1:t1" in rails, str(rails))
check("remote chat follows the space's local chats",
      rails.index("tab:ax42/w1:t1") > max(rails.index("tab:s1:t1"), rails.index("tab:s1:t2")) if "tab:ax42/w1:t1" in rails else False, str(rails))
check("no second Rails space", at("space:ax42/w1") == -1)
chat = fields("tab:ax42/w1:t1")
check("remote chat carries its machine badge", chat is not None and chat[-1] == "@ax42", str(chat))
check("remote chat keeps its state glyph", chat is not None and chat[4:6] == ["●", "working"], str(chat))
check("local rows carry no badge", all("@" not in r.split("|")[-1] for r in rows if "ax42/" not in r and "pc/" not in r))
group = at("spacegroup:Rails")
own = at("space:ax42/w2")
check("unmatched remote space stands on its own", own >= 0 and fields("tab:ax42/w2:t1") is not None and fields("tab:ax42/w2:t1")[-1] == "@ax42")
check("unmatched remote space sits in its areas group after the local member",
      group >= 0 and at("space:s1") > group and own > at("space:s1") and own < at("space:s2"),
      f"group {group} rails {at('space:s1')} recruiting {own} poker {at('space:s2')}")
check("chatless remote tabs and spaces show nothing",
      not any(x in r for r in rows for x in ("ax42/w1:t2", "ax42/w3", "forge")))
pins = [r.split("|")[1] for r in rows if r.startswith("tab|pinned:")]
check("remote pins follow local pins", pins == ["pinned:s1:t1", "pinned:ax42/w1:t1"], str(pins))
pinned = fields("pinned:ax42/w1:t1")
check("remote pinned row keeps the space label and gets the badge", pinned is not None and pinned[7] == "rails" and pinned[-1] == "@ax42", str(pinned))
check("pinned header counts every pin", (fields("pinned") or [""] * 8)[7] == "⌘1..2", str(fields("pinned")))
pc = fields("tab:pc/w1:t1")
check("unreachable machine's chat sits in its space with a dimmed badge state",
      "tab:pc/w1:t1" in block("s2") and pc is not None and pc[-1] == "@pc:unreachable", str(pc))
stripped = [r for r in rows if not ("ax42/" in r or "pc/" in r) and not r.startswith("section|pinned|")]
base = [r for r in local if not r.startswith("section|pinned|")]
check("local rows are unchanged by the merge", stripped == base,
      "\n" + "\n".join(f"- {x}\n+ {y}" for x, y in zip(base, stripped) if x != y))
print("\n".join(rows))
(ROOT / "checks/MACHINES-MERGE.txt").write_text("\n".join(lines + [""] + rows) + "\n")
raise SystemExit(1 if failures else 0)
