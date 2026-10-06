#!/usr/bin/env python3
"""A stand-in for Claude Code in a lab pane (check_p19.py). Shows a `❯` input line,
reads what the terminal delivers, and writes each line to the transcript as a human
prompt, the way Claude Code records typed input. Two control lines from the check:
`__block__` draws Claude's permission dialog (herdr's claude manifest then reports the
pane blocked) and `__unblock__` clears it. Usage: p19_fake_claude.py TRANSCRIPT"""
import json
import sys
import uuid

DIALOG = """\x1b[2J\x1b[H────────────────────────────────────────
 Bash command

   rm -rf build

 Do you want to proceed?
 ❯ 1. Yes
   2. No, and tell Claude what to do differently (esc)

 Esc to cancel · Tab to amend · ctrl+e to explain
"""

path = sys.argv[1]
while True:
    try:
        line = input('❯ ')
    except EOFError:
        break
    if line == '__block__':
        print(DIALOG, flush=True)
        continue
    if line == '__unblock__':
        print('\x1b[2J\x1b[H', end='', flush=True)
        continue
    print('received: ' + line, flush=True)
    with open(path, 'a') as f:
        f.write(json.dumps({'uuid': str(uuid.uuid4()), 'type': 'user', 'origin': {'kind': 'human'}, 'message': {'role': 'user', 'content': line}}) + '\n')
