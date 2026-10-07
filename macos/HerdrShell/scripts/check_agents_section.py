#!/usr/bin/env python3
"""Pure sidebar/shortcut ordering contract across decoded machine snapshots.

The file/subprocess boundary exercises decoding, merge and the renderer without an app.
Existing parity fixtures have no roles: they cannot catch agents interleaved after plain pins,
duplicate agent rows or key hints on the AGENTS and PINNED headers. Agent faces read real agent.json files from a
temp dir and are held to an independent port of Rails personTint, so the two apps agree. Uses the production pin-order algorithm;
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
                    str(ROOT / 'Sources/HerdrShell/AgentCards.swift'),
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

    # JS String.prototype.trim: WhiteSpace (Zs, tab, VT, FF, BOM) and LineTerminator, not U+0085.
    JS_SPACE = '\t\x0b\x0c \xa0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200a\u202f\u205f\u3000\ufeff\n\r\u2028\u2029'

    def tint(name):
        # agent-rails apps/workspace/src/ui/person-face.tsx personTint, nine tints.
        h = 7
        for ch in name.strip(JS_SPACE).lower():
            h = (h * 31 + ord(ch)) & 0xFFFFFFFF
        return h % 9

    def face(row_id):
        return next((f for f in field(row_id) or [] if f.startswith('face:')), None)

    check('no agent cards: agent rows still get tinted initials from the label',
          face('agent:ax42/w1:t1') == f"face:R:{tint('remote chat')}" and field('agent:ax42/w1:t1')[-1] == '@ax42')
    check('plain pinned and space rows carry no face', face('pinned:s1:t1') is None and face('tab:s1:t1') is None)
    odd = '\ufeffe\u0301mile\u3000'
    unicode_fixture = json.loads(json.dumps(fixture))
    unicode_fixture['machines'][0]['tabs'][0]['label'] = odd
    rows = dump(unicode_fixture)
    check('initial and tint follow JS trim and slice(0, 1) on odd Unicode',
          face('agent:ax42/w1:t1') == f"face:E:{tint(odd)}")
    zwsp = '\u200bAlpha'
    unicode_fixture['machines'][0]['tabs'][0]['label'] = zwsp
    rows = dump(unicode_fixture)
    check('a zero-width space survives trim, as in JS', face('agent:ax42/w1:t1') == f"face:\u200b:{tint(zwsp)}")
    agents = pathlib.Path(directory) / 'agents'
    cards = {'frank': {'name': 'frank', 'pane': 'w5H:p137', 'avatar_url': 'https://example.com/frank.png'},
             'recruiter': {'name': 'Recruiter', 'pane': 'w5P:p9', 'avatar_url': 'http://example.com/r.png'},
             'p7probe': {'name': 'p7probe', 'pane': 'none'}}
    for name, card in cards.items():
        (agents / name).mkdir(parents=True)
        (agents / name / 'agent.json').write_text(json.dumps(card))
    (agents / 'broken').mkdir()
    (agents / 'broken' / 'agent.json').write_text('{not json')
    carded = json.loads(json.dumps(fixture))
    for tab in carded['input']['tabs'][:2]:
        tab['role'] = 'agent'
    carded['agentsDir'] = str(agents)
    carded['panes'] = [['w5H:p137', 's1:t1'], ['w5P:p9', 's1:t2'], ['w5H:p138', 's1:t2'], ['w5H:pZF', 's2:t1']]
    rows = dump(carded)
    check('agent.json name and https avatar_url give the face of the tab holding its pane',
          face('agent:s1:t1') == f"face:F:{tint('frank')}:https://example.com/frank.png")
    check('a non-https avatar_url falls back to tinted initials of the card name',
          face('agent:s1:t2') == f"face:R:{tint('Recruiter')}")
    check('a card on a plain tab, a pane-less card and a broken card add nothing',
          face('tab:s2:t1') is None and 'p7probe' not in '\n'.join(rows) and len([r for r in rows if 'face:' in r]) == 3)
    check('the status glyph and tone still ride the agent row', field('agent:s1:t1')[4:6] == ['●', 'working'])
    rows = original
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
