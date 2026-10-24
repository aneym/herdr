#!/usr/bin/env python3
"""Settings > Agents row contract, through the real SpacesInput decoder and SettingsAgents.

Compiles the production Swift (no test seam) and feeds it a decoded fixture: every agent pin in
pin order with hidden ones kept in their slot as switched off, plain pins and plain tabs left out,
space and runs-on as the meta line, the Rails face tint, and a pending toggle that holds until the
server echoes it. The Windows twin is app/src/settingsAgents.test.tsx; both read the same fixture.
Writes checks/SETTINGS-AGENTS.txt.
"""
import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
FIXTURE = REPO / 'shell/fixtures/settings-agents.json'
lines, failures = [], []


def check(name, ok, detail=''):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}" + (f' ({detail})' if detail and not ok else '')
    print(line)
    lines.append(line)
    if not ok:
        failures.append(name)


with tempfile.TemporaryDirectory(prefix='settings-agents-') as directory:
    driver = pathlib.Path(directory) / 'dump'
    sources = ['SpacesTree.swift', 'MachineMerge.swift', 'SettingsAgents.swift']
    subprocess.run(['swiftc', *[str(ROOT / 'Sources/HerdrShell' / s) for s in sources],
                    str(ROOT / 'scripts/settings_agents_dump.swift'), '-o', str(driver)], check=True)

    def dump(fixture):
        path = pathlib.Path(directory) / 'fixture.json'
        path.write_text(json.dumps(fixture))
        out = subprocess.check_output([str(driver), str(path)], text=True).splitlines()
        rows = [l.split('|')[1:] for l in out if l.startswith('row|')]
        settled = next(l for l in out if l.startswith('settled|')).split('|', 1)[1]
        return rows, [s for s in settled.split(',') if s]

    fixture = json.loads(FIXTURE.read_text())
    expected = fixture['expected']
    rows, settled = dump({'input': fixture['input']})
    check('agent pins in pin order; plain pins and tabs left out', [r[0] for r in rows] == [e['tab'] for e in expected], str(rows))
    check('titles prefer the agent card name, else the tab label', [r[1] for r in rows] == [e['title'] for e in expected], str(rows))
    check('meta reads space, then where it runs', [r[2] for r in rows] == [e['meta'] for e in expected], str(rows))
    check('a hidden agent keeps its slot and reads switched off', [r[3] for r in rows] == [str(e['shown']).lower() for e in expected], str(rows))
    check('faces match the Rails tint', [r[4] for r in rows] == [e['face'] for e in expected], str(rows))

    hidden = next(e['tab'] for e in expected if not e['shown'])
    shown = next(e['tab'] for e in expected if e['shown'])
    rows, settled = dump({'input': fixture['input'], 'pending': {hidden: True, shown: False}})
    by = {r[0]: r[3] for r in rows}
    check('a pending toggle wins until the server echoes it', by[hidden] == 'true' and by[shown] == 'false', str(rows))
    check('pending toggles the server has not applied stay', sorted(settled) == sorted([hidden, shown]), str(settled))
    echoed = json.loads(json.dumps(fixture['input']))
    for tab in echoed['tabs']:
        if tab['id'] == hidden:
            tab['hidden'] = False
    rows, settled = dump({'input': echoed, 'pending': {hidden: True, shown: False}})
    check('an echoed toggle settles; the other still waits', settled == [shown], str(settled))

(ROOT / 'checks').mkdir(exist_ok=True)
(ROOT / 'checks/SETTINGS-AGENTS.txt').write_text('\n'.join(lines) + '\n')
if failures:
    raise SystemExit('FAIL: ' + ', '.join(failures))
