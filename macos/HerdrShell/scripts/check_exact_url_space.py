#!/usr/bin/env python3
"""Real terminal clicks preserve URL bytes on the way to Aside.

The table guards query/fragment detection. No production test seam is added.
Run HERDR_SHELL_SPACE=1 python3 scripts/check_exact_url_space.py --out DIR.
"""
import argparse
import json
import os
import pathlib
import shlex
import time

if os.environ.get('HERDR_SHELL_SPACE') != '1':
    raise SystemExit('isolated Space required')
os.environ['SHELL_LAB'] = 'shellspike-eu'
os.environ['SHELL_LAB_DIR'] = os.path.expanduser('~/.cache/hs-eu')
os.environ['HERDR_SPACE_OWNER'] = 'herdr-link-fix'
os.environ.setdefault('HERDR_SHELL_BIN', os.path.expanduser('~/.local/bin/herdr'))
import scenario as S

args = argparse.ArgumentParser()
args.add_argument('--out', required=True)
out = pathlib.Path(args.parse_args().out)
out.mkdir(parents=True, exist_ok=True)
S.STATE = str(out / 'state.json')
EXACT = 'https://app.rails.so/workspace/rails-admin?route=scoping/agent-ui'
CASES = [(EXACT, EXACT),
         ('https://example.com/docs#section', 'https://example.com/docs#section'),
         ('https://example.com/?q=a%2Fb&next=%3Froute%3Dx#fragment', 'https://example.com/?q=a%2Fb&next=%3Froute%3Dx#fragment'),
         ('https://example.com/trailing?x=1).', 'https://example.com/trailing?x=1')]
lines, failures = [], []


def check(name, condition, detail=''):
    line = f"[{'PASS' if condition else 'FAIL'}] {name}: {detail}"
    print(line, flush=True)
    lines.append(line)
    if not condition:
        failures.append(name)


def wait(predicate, timeout=25):
    deadline = time.monotonic() + timeout
    state = {}
    while time.monotonic() < deadline:
        state = S.state()
        if predicate(state):
            return state
        time.sleep(.2)
    return state


def api(*args):
    return json.loads(S.lab('herdr', *args))['result']


try:
    S.lab('up')
    made = api('workspace', 'create', '--label', 'exact-url', '--no-focus')
    tab, pane = made['tab']['tab_id'], made['root_pane']['pane_id']
    S.app('start')
    S.cmd({'cmd': 'activate'})
    S.cmd({'cmd': 'select', 'tab': tab})

    def surface(state):
        return next((s for s in state.get('surfaces', []) if s['pane'] == pane), {})

    wait(lambda s: surface(s).get('cols', 0) > 0)
    S.wait_read(pane, lambda text: '%' in text, timeout=10)
    command = "printf '\\033[2J\\033[H'; printf '%s\\n' " + ' '.join(shlex.quote(raw) for raw, _ in CASES) + '; sleep 3600'
    S.lab('herdr', 'pane', 'run', pane, command)
    wait(lambda s: any(line.startswith('https://app.rails.so/') for line in surface(s).get('visible_nonblank', [])))

    def click(url):
        deadline = time.monotonic() + 10
        row = None
        while time.monotonic() < deadline:
            visible = S.lab('herdr', 'pane', 'read', pane, '--source', 'visible').splitlines()
            row = next((i for i, text in enumerate(visible) if text.startswith(url[:25])), None)
            if row is not None:
                break
            time.sleep(.2)
        if row is None:
            raise RuntimeError('URL row missing: ' + repr(visible))
        pointer = {'cmd': 'mouse', 'pane': pane, 'col': 10., 'row': float(row), 'mods': ['cmd']}
        S.cmd({**pointer, 'action': 'move'})
        before = len(S.state().get('opened_urls', []))
        S.cmd({**pointer, 'action': 'down'})
        S.cmd({**pointer, 'action': 'up'})
        return wait(lambda s: len(s.get('opened_urls', [])) > before)

    for raw, url in CASES:
        state = click(raw)
        opened = state.get('opened_urls', [])
        check('terminal opens exact target in Aside ' + url, bool(opened) and opened[-1] == 'aside ' + url, json.dumps(opened[-1:]))
except (Exception, SystemExit) as error:
    check('scenario completed', False, str(error))
finally:
    S.app('stop')
    S.lab('down')
    (out / 'check.txt').write_text('\n'.join(lines) + '\n')
print(f'{len(failures)} failed', flush=True)
raise SystemExit(bool(failures))
