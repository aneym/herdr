#!/usr/bin/env python3
"""Cua-only Finder folder drop: the unfocused pane receives escaped, unsent text.
Uses a private pasteboard through the terminal's AppKit drop handler, not a fake
send or the system clipboard. Reads the real server detection buffer for proof.
"""
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

D = Path(__file__).resolve().parent.parent
os.environ['SHELL_LAB'] = 'shellspike-drop'
os.environ['HERDR_SHELL_SPACE'] = '1'
os.environ['HERDR_SPACE_OWNER'] = 'drop-paths-mac'
os.environ['HERDR_SHELL_BIN'] = os.environ.get('HERDR_SHELL_BIN', os.path.expanduser('~/.local/bin/herdr'))
os.environ['HERDR_SHELL_APP'] = str(D / '.build/debug/HerdrShell')
sys.path.insert(0, str(D / 'scripts'))
import scenario as S


def main():
    lines = []
    import lab
    def agent_read(pane):
        result = subprocess.run([lab.BIN, '--session', lab.SESSION, 'agent', 'read', pane, '--source', 'detection', '--format', 'text'], env=lab.env(), capture_output=True, text=True, check=True)
        return result.stdout
    def say(text):
        print(text, flush=True)
        lines.append(text)
    try:
        with tempfile.TemporaryDirectory() as tmp:
            exe = str(Path(tmp) / 'file-drop-tests')
            subprocess.run(['swiftc', str(D / 'Sources/HerdrShell/FileDropPaths.swift'),
                            str(D / 'scripts/file_drop_paths.swift'), '-o', exe], check=True)
            say(subprocess.check_output([exe], text=True).strip())
        S.lab('up')
        snap = json.loads(S.lab('herdr', 'api', 'snapshot'))['result']['snapshot']
        tab = next(x['tab_id'] for x in snap['tabs'] if x['label'] == 'shell spike')
        layout = next(x for x in snap['layouts'] if x['tab_id'] == tab)
        panes = sorted(layout['panes'], key=lambda x: x['rect']['x'])
        first, target = panes[0]['pane_id'], panes[1]['pane_id']
        say(S.app('start').strip())
        S.cmd({'cmd': 'select', 'tab': tab})
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            state = S.state()
            if state.get('focused_pane') == first and len(state.get('surfaces', [])) >= 2:
                break
            time.sleep(.2)
        else:
            raise AssertionError('lab pane did not become ready')
        for pane in (first, target):
            S.lab('herdr', 'pane', 'report-agent', pane, '--source', 'drop-check', '--agent', 'claude', '--state', 'idle')
        # Put the prompt at the bottom so the detector's bottom-buffer source
        # includes it after the attach client changes the terminal geometry.
        S.lab('herdr', 'pane', 'run', target, "printf 'lab-ready\\n%.0s' {1..100}")
        time.sleep(.5)
        S.space('exec', "mkdir -p '/tmp/drop test dir'")
        S.cmd({'cmd': 'drop_files', 'pane': target, 'paths': ['/tmp/drop test dir']})
        deadline = time.monotonic() + 15
        expected = '/tmp/drop\\ test\\ dir '
        while time.monotonic() < deadline:
            text = agent_read(target)
            if expected.rstrip() in text:
                break
            time.sleep(.2)
        else:
            raise AssertionError('escaped unsent folder path missing: ' + text)
        state = S.state()
        assert state['focused_pane'] == target, state['focused_pane']
        other = agent_read(first)
        assert expected.rstrip() not in other, 'drop reached previously focused pane'
        say('PASS: drop focuses the target pane, not the previously focused pane')
        say(f'herdr agent read {target} --source detection --format text:')
        say(text)
        say('Note: agent-read text trims terminal trailing blanks; golden escaping cases verify the inserted trailing space.')
        assert 'no such file' not in text.lower() and 'permission denied' not in text.lower()
        say('PASS: /tmp/drop\\ test\\ dir [trailing space] remains unsent in the prompt')
        for mode in ('light', 'dark'):
            S.cmd({'cmd': 'appearance', 'mode': mode})
            time.sleep(.3)
            shot = os.path.expanduser(f'~/.agent-rails/lanes/herdr-ui/evidence/drop-paths-mac-{mode}.png')
            S.cmd({'cmd': 'shot', 'out': shot})
            say('artifact: ' + shot)
        say('RESULT: PASS')
    finally:
        S.app('stop')
        S.lab('down')
        out = Path(os.path.expanduser('~/.agent-rails/lanes/herdr-ui/evidence/drop-paths-mac.txt'))
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text('\n'.join(lines) + '\n')


if __name__ == '__main__':
    main()
