#!/usr/bin/env python3
"""Real terminal clicks preserve URL bytes and reopen a reused panel after navigation.

The table guards query/fragment detection; the reuse case guards the desk reply /
snapshot lifecycle that parser tests cannot reach. No production test seam is added.
Run HERDR_SHELL_SPACE=1 python3 scripts/check_exact_url_space.py --out DIR.
"""
import argparse
import http.server
import json
import os
import pathlib
import shlex
import threading
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


requests = []


class RedirectPage(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        requests.append(self.path)
        body = b'<meta http-equiv="refresh" content="0;url=/away">' if self.path.startswith('/start') else b'<h1>Navigation away</h1>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


server = http.server.ThreadingHTTPServer(('0.0.0.0', 0), RedirectPage)
threading.Thread(target=server.serve_forever, daemon=True).start()
reused = f'http://192.168.64.1:{server.server_port}/start?q=a%2Fb#fragment'
CASES.append((reused, reused))

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
    # Output stays fixed while the side panel changes the terminal width.
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
        check('terminal opens exact target ' + url, bool(opened) and opened[-1] == 'desk ' + url, json.dumps(opened[-1:]))
        desk = json.loads(S.lab('herdr', 'desk', 'list', '--tab', tab, '--json'))
        check('desk retains exact target ' + url, any(i['ref'] == url for d in desk['desks'] for i in d['items']))
        if url == EXACT:
            time.sleep(5)
            state = S.state()
            lines.append('Rails panel URL readback: ' + state.get('docs', {}).get('url', ''))
            S.cmd({'cmd': 'shot', 'out': str(out / 'rails-url.png')})

    state = wait(lambda s: s.get('docs', {}).get('url', '').endswith('/away'))
    check('page navigated existing panel away', state.get('docs', {}).get('url', '').endswith('/away'), state.get('docs', {}).get('url', ''))
    before = requests.count('/start?q=a%2Fb')
    click(reused)
    wait(lambda s: requests.count('/start?q=a%2Fb') > before)
    check('reopening existing item requests full URL again', requests.count('/start?q=a%2Fb') == before + 1, repr(requests))
    S.cmd({'cmd': 'shot', 'out': str(out / 'reopened-url.png')})
except (Exception, SystemExit) as error:
    check('scenario completed', False, str(error))
finally:
    S.app('stop')
    S.lab('down')
    server.shutdown()
    server.server_close()
    (out / 'check.txt').write_text('\n'.join(lines) + '\n')
print(f'{len(failures)} failed', flush=True)
raise SystemExit(bool(failures))
