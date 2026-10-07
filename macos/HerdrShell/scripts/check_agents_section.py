#!/usr/bin/env python3
"""Pure sidebar/shortcut ordering contract across decoded machine snapshots.

The file/subprocess boundary exercises decoding, merge and the renderer without an app.
Existing parity fixtures have no roles: they cannot catch agents interleaved after plain pins,
duplicate agent rows or key hints on the AGENTS and PINNED headers. Uses the production pin-order algorithm;
no test-only production seam. UI gestures remain a Cua Space proof owned by the lane.
"""
import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'scripts/fixtures/agents/agents.json'
lines, failures = [], []


def check(name, ok):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}"
    print(line)
    lines.append(line)
    if not ok:
        failures.append(name)


with tempfile.TemporaryDirectory(prefix='agents-section-') as directory:
    driver = pathlib.Path(directory) / 'dump'
    subprocess.run(['swiftc', str(ROOT / 'Sources/HerdrShell/SpacesTree.swift'),
                    str(ROOT / 'Sources/HerdrShell/MachineMerge.swift'),
                    str(ROOT / 'scripts/agents_section_dump.swift'), '-o', str(driver)], check=True)

    def dump(fixture):
        path = pathlib.Path(directory) / 'fixture.json'
        path.write_text(json.dumps(fixture))
        return subprocess.check_output([str(driver), str(path)], text=True).splitlines()

    fixture = json.loads(FIXTURE.read_text())
    rows = dump(fixture)
    def field(row_id):
        return next((r.split('|') for r in rows if r.split('|')[1] == row_id), None)
    def at(row_id):
        return next((i for i, r in enumerate(rows) if r.split('|')[1] == row_id), -1)
    check('agents title row omitted above the AGENTS section', at('agents') == -1 and rows[2].split('|')[1] == 'agentpins')
    check('AGENTS precedes PINNED (remote agent before local plain pin)', 0 <= at('agentpins') < at('agent:ax42/w1:t1') < at('pinned') < at('pinned:s1:t1'))
    check('agent appears once, only in AGENTS', sum('ax42/w1:t1' in r.split('|')[1] for r in rows if r.startswith('tab|')) == 1 and at('tab:ax42/w1:t1') == -1)
    check('AGENTS and PINNED headers carry no key hints (Alex, 2026-10-06)', field('agentpins')[7] == '' and field('pinned')[7] == '')
    check('remote agent badge and space label survive', field('agent:ax42/w1:t1')[-1] == '@ax42' and field('agent:ax42/w1:t1')[7] == 'rails')
    check('numbered order starts remote A, local P', rows[0].split('|')[1].split(',')[:2] == ['ax42/w1:t1', 's1:t1'])
    check('source pin indices preserved for moves', rows[1] == 'indices|s1:t1:0,ax42/w1:t1:0')
    original = rows[:]
    fixture['input']['tabs'][0]['role'] = 'agent'
    rows = dump(fixture)
    check('all-agent pins omit PINNED', at('pinned') == -1 and at('agentpins') >= 0)
    for machine in [fixture['input']] + fixture['machines']:
        for tab in machine['tabs']:
            tab.pop('role', None)
    rows = dump(fixture)
    check('older snapshots have no AGENTS section and keep the agents title row', at('agentpins') == -1 and rows[2].split('|')[1] == 'agents')
    fixture['input']['tabs'][0]['role'] = 'future-role'
    rows = dump(fixture)
    check('unknown roles remain plain', at('agentpins') == -1 and at('pinned:s1:t1') >= 0)
(ROOT / 'checks').mkdir(exist_ok=True)
(ROOT / 'checks/AGENTS-SECTION.txt').write_text('\n'.join(lines + [''] + original) + '\n')
raise SystemExit(1 if failures else 0)
