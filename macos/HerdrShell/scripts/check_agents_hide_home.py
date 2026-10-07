#!/usr/bin/env python3
"""Hide agents and the home glyph on agent rows, over the Mac Shell's own rows (spec
agents-hide-and-home-glyph-2026-10-07, slice M).

Runs the production tree, snapshot decoder, machine merge and command transport through the
Swift interpreter with scripts/agents_hide_home_dump.swift: no app, no herdr server, and no
newly built executable (a never-run binary can hang at _dyld_start on the Studio). Other
machines arrive as raw session.snapshot JSON through Machines.namespace and MachineRows, and
tab.set_hidden goes over real Unix sockets to a disposable listener, so wire names and routing
are what the Shell sends. The story is the same as the Rust scenario: agent pins A, B, C and
plain pin P; B hidden, then shown again in its old slot. RowMenu, PinDrag and numberedTabIds
need the whole app to compile, so their pure rules live in SpacesTree and the last check ties
the app code to them. Glyph drawing, colors and tooltips are the lane's screenshot pass.
Writes checks/AGENTS-HIDE-HOME.txt.
"""
import json
import os
import pathlib
import re
import socket
import subprocess
import tempfile
import threading

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / 'Sources/HerdrShell'
FIXTURE = ROOT / 'scripts/fixtures/agents/hide-home.json'
# HerdrClient needs the socket, snapshot and machine files; `log` lives in the app's Ghostty file.
FILES = ['SpacesTree.swift', 'MachineMerge.swift', 'DeskModel.swift', 'Snapshot.swift', 'Machines.swift',
         'HerdrSocket.swift', 'HerdrClient.swift', 'PaneRestart.swift']
A, B, C, P, COACH = 's1:t1', 's1:t2', 's2:t1', 's1:t3', 's2:t2'
R1, R2, R3 = 'ax42/w1:t1', 'ax42/w1:t2', 'ax42/w1:t3'
lines, failures = [], []


def check(name, ok, detail=''):
    line = f"[{'PASS' if ok else 'FAIL'}] {name}" + (f' ({detail})' if detail and not ok else '')
    print(line, flush=True)
    lines.append(line)
    if not ok:
        failures.append(name)


def listen(directory, name, replies, calls):
    """A one-line JSON API on <directory>/<name> answering each connection with the next reply."""
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    previous = os.getcwd()
    os.chdir(directory)  # AF_UNIX paths are short; bind relative to the scratch dir.
    try:
        server.bind(name)
    finally:
        os.chdir(previous)
    server.listen(len(replies))
    server.settimeout(300)  # the interpreter compiles the sources before it connects

    def serve():
        try:
            for reply in replies:
                conn, _ = server.accept()
                with conn:
                    calls.append(json.loads(conn.makefile('rb').readline()))
                    conn.sendall((json.dumps(reply) + '\n').encode())
        except OSError:
            pass
        finally:
            server.close()
    worker = threading.Thread(target=serve, daemon=True)
    worker.start()
    return worker


fixture = json.loads(FIXTURE.read_text())


def variant(name, local_only=True, chrome=None, toggles=None, tabs=None, select=None, space=None):
    """The fixture with B's tab (or any tab) patched; local_only drops the other machine."""
    out = json.loads(json.dumps(fixture))
    out['_variant'] = name
    if local_only:
        out['remote'] = []
    if chrome is not None:
        out['chrome'] = chrome
    if toggles:
        out['toggles'] = toggles
    for tab in out['input']['tabs']:
        tab.update((tabs or {}).get(tab['id'], {}))
        for key, value in list(tab.items()):
            if value is None:
                del tab[key]
    if select:
        out['select'], out['selectSpace'] = select, space
    return name, out


variants = dict([
    variant('shut'),
    variant('open', chrome={'hiddenAgentsExpanded': True}),
    variant('toggled', toggles=['hiddenagents']),
    variant('spaces-fold', chrome={'hiddenExpanded': True}),
    variant('shown', tabs={B: {'hidden': None}}),
    variant('all-hidden', tabs={A: {'hidden': True}, C: {'hidden': True}}),
    variant('blocked', tabs={B: {'work': 'blocked', 'status': 'blocked', 'agents': [{'status': 'blocked'}]}}),
    variant('request', tabs={B: {'request': 'req-1'}}),
    variant('working', tabs={B: {'work': 'working', 'status': 'working', 'agents': [{'status': 'working'}]}}),
    variant('done', tabs={B: {'work': 'done', 'status': 'done', 'agents': [{'status': 'done'}]}}),
    variant('blocked-open', chrome={'hiddenAgentsExpanded': True},
            tabs={B: {'work': 'blocked', 'status': 'blocked', 'agents': [{'status': 'blocked'}]}}),
    variant('select-hidden', chrome={'hiddenAgentsExpanded': True, 'collapsedSpaces': ['s1']}, select=B, space='s1'),
    variant('machines', local_only=False),
    variant('machines-open', local_only=False, chrome={'hiddenAgentsExpanded': True}),
])

with tempfile.TemporaryDirectory(prefix='hide-home-', dir=os.environ.get('TMPDIR')) as tmp:
    scratch = pathlib.Path(tmp)
    source = scratch / 'combined.swift'
    source.write_text('import Foundation\nfunc log(_ message: String) {}\n'
                      + '\n'.join((SRC / name).read_text() for name in FILES)
                      + '\n' + (ROOT / 'scripts/agents_hide_home_dump.swift').read_text())
    paths = []
    for name, content in variants.items():
        path = scratch / (name + '.json')
        path.write_text(json.dumps(content))
        paths.append(str(path))
    local_calls, remote_calls = [], []
    tab_info = {'result': {'type': 'tab_info', 'tab': {'tab_id': 'w1:t2', 'hidden': True}}}
    unknown = {'error': {'code': 'unknown_method', 'message': 'unknown method tab.set_hidden'}}
    workers = [listen(tmp, 'api.sock', [tab_info, unknown], local_calls),
               listen(tmp, 'ax42.sock', [{'result': {'type': 'tab_info', 'tab': {'tab_id': 'w1:t2'}}}], remote_calls)]
    tunnels = scratch / 'tunnels.json'
    tunnels.write_text(json.dumps({'machines': [{'name': 'ax42', 'socket': 'ax42.sock'}]}))  # relative to cwd
    run = subprocess.run(['swift', '-module-cache-path', str(scratch / 'modules'), str(source),
                          'api.sock', str(tunnels)] + paths,
                         cwd=tmp, capture_output=True, text=True, timeout=240)
    for worker in workers:
        worker.join(timeout=5)
    if run.returncode != 0:
        print(run.stdout)
        print(run.stderr)
        (ROOT / 'checks').mkdir(exist_ok=True)
        (ROOT / 'checks/AGENTS-HIDE-HOME.txt').write_text('[FAIL] driver did not compile or run\n' + run.stderr[-4000:] + '\n')
        raise SystemExit(1)

dumps, current, transport = {}, None, None
for out_line in run.stdout.splitlines():
    if out_line.startswith('== '):
        current = pathlib.Path(out_line[3:]).stem
        dumps[current] = []
    elif out_line.startswith('transport|'):
        transport = out_line.split('|')[1:]
    elif current:
        dumps[current].append(out_line)

KINDS = ('title', 'goal', 'space', 'section', 'group', 'tab', 'run', 'hidden', 'footerUsage', 'footerHost')
MISSING = ['?'] * 13  # a row the dump lacks: each field check then fails instead of raising


def rows(name):
    return [l.split('|') for l in dumps.get(name, []) if l.split('|')[0] in KINDS]


def ids(name):
    return [r[1] for r in rows(name)]


def row(name, row_id):
    return next((r for r in rows(name) if r[1] == row_id), MISSING)


def line(name, kind):
    return next((l.split('|')[1:] for l in dumps.get(name, []) if l.startswith(kind + '|')), ['?'])


def numbered(name):
    return line(name, 'numbered')[0].split(',')


def tagged(name, kind):
    return {l.split('|')[1]: l.split('|')[2] for l in dumps.get(name, []) if l.startswith(kind + '|')}


def extras(r):
    return r[13:]


def home(r):
    return next((f for f in extras(r) if f.startswith('home:')), None)


def dot(r):
    return next((f for f in extras(r) if f.startswith('dot:')), None)


def agent_block(name):
    """Row ids from the AGENTS header through the last PINNED row."""
    out = ids(name)
    pinned = [i for i, x in enumerate(out) if x.startswith('pinned:')]
    return out[out.index('agentpins'):pinned[-1] + 1] if 'agentpins' in out and pinned else out


# The story, shut: AGENTS, A, C, the Hidden header with B's count, PINNED, P.
header = row('shut', 'hiddenagents')
check('B hidden: AGENTS, A, C, Hidden, PINNED, P in that order',
      agent_block('shut') == ['agentpins', 'agent:' + A, 'agent:' + C, 'hiddenagents', 'pinned', 'pinned:' + P],
      ','.join(ids('shut')[:8]))
check('no agents title row above the AGENTS section', 'agents' not in ids('shut'))
check('the Hidden header is one shut row: kind hidden, depth 0, title Hidden, count 1, toggles hiddenagents',
      header[0] == 'hidden' and header[2] == '0' and header[3] == 'closed'
      and header[6] == 'Hidden' and header[7] == '1' and header[11] == 'hiddenagents', '|'.join(header))
check('shut fold draws no B row, and B never shows in its space',
      not any(B in x for x in ids('shut')) and 'tab:' + B not in ids('open'))
check('digits A1 C2 P3: numbered skips hidden B (shut and open)',
      numbered('shut')[:3] == [A, C, P] and B not in numbered('shut')
      and numbered('open')[:3] == [A, C, P] and B not in numbered('open'), ','.join(numbered('open')))

# Open: B at depth 1 under the header, drawn as an agent row, still with no digit.
hidden_b = row('open', 'hiddenagent:' + B)
check('open fold: hiddenagent:B right under the header, before PINNED',
      agent_block('open') == ['agentpins', 'agent:' + A, 'agent:' + C, 'hiddenagents', 'hiddenagent:' + B, 'pinned', 'pinned:' + P])
check('open header chevron is open with the same count',
      row('open', 'hiddenagents')[3] == 'open' and row('open', 'hiddenagents')[7] == '1')
check('hidden row: tab kind, depth 1, opens B, agent face, no space name',
      hidden_b[0] == 'tab' and hidden_b[2] == '1' and hidden_b[10] == B and hidden_b[7] == ''
      and any(f.startswith('face:') for f in extras(hidden_b)), '|'.join(hidden_b))
check('the header toggle key opens the fold and the choice survives a save',
      ids('toggled') == ids('open') and line('toggled', 'chrome') == ['true', 'false'] and line('shut', 'chrome') == ['false', 'false'])
check('the hidden-spaces fold does not open hidden agents',
      row('spaces-fold', 'hiddenagents')[3] == 'closed' and 'hiddenagent:' + B not in ids('spaces-fold'))

# Unhide: B is back in slot 2, and the Hidden section is gone.
check('unhidden B returns to slot 2 and the Hidden header goes',
      agent_block('shown') == ['agentpins', 'agent:' + A, 'agent:' + B, 'agent:' + C, 'pinned', 'pinned:' + P]
      and numbered('shown')[:4] == [A, B, C, P])
check('every agent hidden: AGENTS stays and holds only the Hidden header',
      agent_block('all-hidden') == ['agentpins', 'hiddenagents', 'pinned', 'pinned:' + P]
      and row('all-hidden', 'hiddenagents')[7] == '3' and numbered('all-hidden')[0] == P)

# The quiet signal: one accent dot on the shut header for blocked or an open request only.
check('shut header carries dot:accent when a hidden agent is blocked', dot(row('blocked', 'hiddenagents')) == 'dot:accent')
check('shut header carries dot:accent when a hidden agent has an open request', dot(row('request', 'hiddenagents')) == 'dot:accent')
check('no header dot for idle, working or done',
      all(row(v, 'hiddenagents')[0] == 'hidden' and dot(row(v, 'hiddenagents')) is None for v in ('shut', 'working', 'done')))
check('open fold: the dot moves to the blocked row, off the header',
      row('blocked-open', 'hiddenagents')[3] == 'open' and dot(row('blocked-open', 'hiddenagents')) is None
      and dot(row('blocked-open', 'hiddenagent:' + B)) == 'dot:accent')

# Home glyph: AGENTS rows dump home:<location> last with no space name; PINNED keeps its space.
check('AGENTS rows dump home:cloud and home:unsynced last, with an empty trailing',
      extras(row('shut', 'agent:' + A))[-1:] == ['home:cloud'] and row('shut', 'agent:' + A)[7] == ''
      and extras(row('shut', 'agent:' + C))[-1:] == ['home:unsynced'] and row('shut', 'agent:' + C)[7] == '')
check('a hidden row dumps home:local with an empty trailing',
      extras(hidden_b)[-1:] == ['home:local'] and hidden_b[7] == '')
check('a PINNED row keeps its space name and draws no home, even with a location',
      row('shut', 'pinned:' + P)[7] == 'rails' and home(row('shut', 'pinned:' + P)) is None)

# Other machines: snapshot JSON carries hidden and home_location; the @machine badge stays.
r1, r2, r3 = row('machines', 'agent:' + R1), row('machines-open', 'hiddenagent:' + R2), row('machines', 'agent:' + R3)
check('a remote agent keeps its @ax42 badge, with home:unsynced kept last',
      extras(r1)[-2:] == ['@ax42', 'home:unsynced'] and r1[7] == '', '|'.join(r1))
check('an agent from an older snapshot (no home_location) dumps no home and no space name',
      r3[0] == 'tab' and home(r3) is None and r3[7] == '' and '@ax42' in extras(r3), '|'.join(r3))
check('a remote hidden agent joins the fold with its badge; an unknown home_location draws nothing',
      row('machines', 'hiddenagents')[7] == '2' and 'agent:' + R2 not in ids('machines')
      and r2[0] == 'tab' and r2[2] == '1' and '@ax42' in extras(r2) and home(r2) is None
      and ids('machines-open').index('hiddenagent:' + B) < ids('machines-open').index('hiddenagent:' + R2), '|'.join(r2))
check('digits across machines: A, C, ax42 visible agents, then P; no hidden agent numbered',
      numbered('machines')[:5] == [A, C, R1, R3, P] and B not in numbered('machines') and R2 not in numbered('machines'),
      ','.join(numbered('machines')))

# Menus, drag, close and reveal over the drawn rows.
menu, drag = tagged('machines-open', 'menu'), tagged('machines-open', 'drag')
want_menu = {'agent:' + A: 'hide', 'agent:' + R1: 'hide', 'hiddenagent:' + B: 'show', 'hiddenagent:' + R2: 'show',
             'pinned:' + P: '', 'tab:' + COACH: '', 'hiddenagents': ''}
check('RowMenu rule: Hide on agent: rows (remote too), Show in Agents on hiddenagent: rows, nothing elsewhere',
      all(menu.get(k, '?') == v for k, v in want_menu.items()), json.dumps(menu))
want_drag = {'hiddenagent:' + B: '', 'hiddenagent:' + R2: '', 'hiddenagents': '', 'agent:' + A: 'agents', 'pinned:' + P: 'pinned'}
check('PinDrag never drags hiddenagent: rows or the header; agent and pinned rows keep their sections',
      all(drag.get(k, '?') == v for k, v in want_drag.items()), json.dumps(drag))
close = line('select-hidden', 'close')[0].split(',')
check('closing a selected hidden agent moves to the other pins, never to itself',
      B not in close and set(close) == {A, C, P}, ','.join(close))
check('selecting a drawn hidden row reveals nothing: its folded space stays folded',
      line('select-hidden', 'reveal') == ['false', 's1'], '|'.join(line('select-hidden', 'reveal')))

# The command: tab.set_hidden {tab_id, hidden}; an unknown method is a quiet false; remote ids route home.
sent = [(c.get('method'), c.get('params')) for c in local_calls]
got = transport or ['?', '?', '?']
check('tabSetHidden sends tab.set_hidden {tab_id, hidden} and reads success',
      got[0] == 'true' and sent[:1] == [('tab.set_hidden', {'tab_id': 'w1:t2', 'hidden': True})], json.dumps(sent))
check('an older server (unknown_method) is a plain false',
      got[1] == 'false' and sent[1:2] == [('tab.set_hidden', {'tab_id': 'w1:t2', 'hidden': False})], json.dumps(sent))
check('a remote tab is shown again on its own machine, with its raw id',
      got[2] == 'true' and [(c.get('method'), c.get('params')) for c in remote_calls] == [('tab.set_hidden', {'tab_id': 'w1:t2', 'hidden': False})],
      json.dumps(remote_calls))


# The app-only wiring that cannot compile here must reach the rules checked above (source ties).
def body(text, start):
    """The brace-balanced body of the first declaration matching `start`."""
    match = re.search(start, text)
    if not match:
        return ''
    i = text.find('{', match.end() - 1)
    depth = 0
    for j in range(max(i, 0), len(text)):
        depth += {'{': 1, '}': -1}.get(text[j], 0)
        if depth == 0:
            return text[i:j + 1]
    return ''


# All sources, so moving a declaration to another file keeps the tie.
sources = '\n'.join(path.read_text() for path in sorted(SRC.glob('*.swift')))
row_menu = body(sources, r'enum RowMenu\b[^{]*\{')
ties = {
    'numberedTabIds uses SpacesTree.numbered': 'SpacesTree.numbered(' in body(sources, r'func numberedTabIds\(state: SidebarState\)[^{]*\{'),
    'setAgentHidden(_:_:) sends tabSetHidden': 'tabSetHidden(tabId:' in body(sources, r'func setAgentHidden\(_ \w+: String, _ \w+: Bool\)[^{]*\{'),
    'RowMenu.hide is "Hide"': re.search(r'\bhide\s*=\s*"Hide"', row_menu) is not None,
    'RowMenu.show is "Show in Agents"': re.search(r'\bshow\s*=\s*"Show in Agents"', row_menu) is not None,
    'RowMenu.items uses setsHidden': 'setsHidden' in body(row_menu, r'static func items\(for'),
    'the menu sends setAgentHidden true and false': re.search(r'setAgentHidden\(\s*\w+\s*,\s*true\s*\)', sources) is not None
    and re.search(r'setAgentHidden\(\s*\w+\s*,\s*false\s*\)', sources) is not None,
    'PinDrag.section(of:) uses SpacesTree.pinSection': 'SpacesTree.pinSection(' in body(sources, r'static func section\(of'),
}
check('app wiring reaches the pure rules: ' + ', '.join(ties), all(ties.values()),
      'missing: ' + '; '.join(k for k, ok in ties.items() if not ok))

(ROOT / 'checks').mkdir(exist_ok=True)
(ROOT / 'checks/AGENTS-HIDE-HOME.txt').write_text('\n'.join(lines + [''] + dumps.get('shut', []) + [''] + dumps.get('machines-open', [])) + '\n')
raise SystemExit(1 if failures else 0)
