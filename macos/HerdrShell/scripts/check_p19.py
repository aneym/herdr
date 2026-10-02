#!/usr/bin/env python3
"""P19 check: the rendered chat for one Claude pane.

  check_p19.py --out checks/P19.txt [--live-pane w5H:pZZ]

1. Fixture transcript (record shapes from real Claude Code transcripts) rendered with
   --transcript: the dump lists exactly the visible items, in order. The light and dark
   screenshots are taken of this fixture before anything is appended to it.
2. Focus mode on the fixture (the default): runs of tool calls fold to one line, a click on
   a run opens it, the live line names the newest running tool, Full opens every run, and
   an unpinned window saves the mode in UserDefaults (the prior value is restored).
3. Appending to a copy of the fixture: each new assistant message is in the dump within
   1.5 s (n=20, p50/p95).
4. A 300 MiB synthetic transcript: time to first render and sampled peak RSS.
5. Lab (SHELL_LAB, default shellspike-ch): a fake Claude in a lab pane (a script named
   `claude`, so herdr detects it as Claude and reads its screen for blocked), a native
   session report pointing at a transcript the fake writes. Real key events go into the
   composer through the app's control FIFO; `herdr pane read` on the lab pane shows what
   arrived. Covers send, the draft warning and the blocked hold.
6. --live-pane: `--demo chat --pane <id> --read-only` on a real Claude pane, captured
   once its transcript renders. Read-only: the sender refuses to send, and no key event
   is sent to that window.
No command in this check sends input to a non-lab pane, and no window it starts is put on
screen or made active: check windows are background-only (activation policy prohibited),
never ordered in, captured in-process with cacheDisplay, and every state read asserts
window_on_screen and app_active are false.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / '.build/release/HerdrShell'
FIX = ROOT / 'scripts/fixtures/p19'
LAB_NAME = os.environ.setdefault('SHELL_LAB', 'shellspike-ch')
LAB = Path.home() / '.cache/herdr-build' / LAB_NAME
if not os.environ.get('HERDR_SHELL_BIN') and not (Path.home() / '.cache/herdr-build/target-pane-attach/release/herdr').exists():
    # The chat needs no --no-escape spike; the installed herdr (copied into the lab) will do.
    os.environ['HERDR_SHELL_BIN'] = str(Path.home() / '.local/bin/herdr')


ANSWER = """## Tightening the chat view

The tool rows were carrying as much weight as the prose, so the eye had nowhere to rest. Three changes bring them back into proportion:

- Tool calls collapse into one quiet line per run, with the count and the tool names.
- Diffs open inline with **red and green line tints** and no gutter boxes.
- Code uses the terminal's font on a subtle surface, like `Theme.tokens.panel`.

| Element | Before | After |
|---|---|---|
| Tool row | 28 pt, ink | 22 pt, muted |
| Body text | 15 pt, 1.3 | 14 pt, 1.5 |
| Composer | boxed twice | one hairline |

```swift
let p = Palette(t: theme.tokens)
Text(item.text).foregroundStyle(p.mute)
```"""


def records():
    """Real record shapes; ids in VISIBLE are the ones the dump must list, in order."""
    tool = lambda i, name, inp: {'type': 'tool_use', 'id': i, 'name': name, 'input': inp}
    result = lambda i, text, error=False: {'type': 'tool_result', 'tool_use_id': i, 'content': text, 'is_error': error}
    return [
        {'uuid': 'human', 'type': 'user', 'origin': {'kind': 'human'}, 'message': {'content': '<pasted_content id="a">\nThe tool rows feel loud and the diff is hard to read. Can you calm the chat view down?\n</pasted_content id="a">'}},
        {'uuid': 'sidechain', 'type': 'assistant', 'isSidechain': True, 'message': {'content': [{'type': 'text', 'text': 'HIDDEN SIDECHAIN'}]}},
        {'uuid': 'meta', 'type': 'user', 'origin': {'kind': 'human'}, 'isMeta': True, 'message': {'content': 'HIDDEN META'}},
        {'uuid': 'compact', 'type': 'user', 'origin': {'kind': 'human'}, 'isCompactSummary': True, 'message': {'content': 'HIDDEN COMPACT'}},
        {'uuid': 'look', 'type': 'assistant', 'message': {'content': [
            {'type': 'thinking', 'thinking': 'HIDDEN THINKING'},
            tool('bash', 'Bash', {'description': 'List the chat sources', 'command': 'ls Sources/HerdrShell'}),
            tool('read', 'Read', {'file_path': '/project/Sources/HerdrShell/ChatView.swift'}),
            tool('grep', 'Grep', {'pattern': 'foregroundStyle', 'path': 'Sources'})]}},
        {'uuid': 'look-results', 'type': 'user', 'message': {'content': [result('bash', 'ChatView.swift\nTranscript.swift'), result('read', '231 lines'), result('grep', 'boom', error=True)]}},
        {'uuid': 'answer', 'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': ANSWER}]}},
        {'uuid': 'edit-turn', 'type': 'assistant', 'message': {'content': [
            tool('edit', 'Edit', {'file_path': '/project/Sources/HerdrShell/ChatView.swift', 'old_string': 'static let toolRow: CGFloat = 28\n.foregroundStyle(tokens.ink)', 'new_string': 'static let toolRow: CGFloat = 22\n.foregroundStyle(p.mute)'})]}},
        {'uuid': 'edit-result', 'type': 'user', 'message': {'content': [result('edit', 'updated')]}},
        {'uuid': 'bulletin', 'type': 'attachment', 'attachment': {'type': 'hook_additional_context', 'content': ['[lane bulletin] info from w5H:p6 (2026-10-01 18:43 ET, topic shell): P18 factory view landed; the chat toggle can sit beside it']}},
        {'uuid': 'duration', 'type': 'system', 'subtype': 'turn_duration', 'durationMs': 84000},
        {'uuid': 'human-2', 'type': 'user', 'origin': {'kind': 'human'}, 'message': {'content': 'Looks right. Run the P19 check and show me the screenshots.'}},
        {'uuid': 'check-turn', 'type': 'assistant', 'message': {'content': [
            tool('build', 'Bash', {'description': 'Build the release app', 'command': 'swift build -c release'}),
            tool('check', 'Bash', {'description': 'Run the P19 check', 'command': 'python3 scripts/check_p19.py --out checks/P19.txt'})]}},
        {'uuid': 'build-result', 'type': 'user', 'message': {'content': [result('build', 'Build complete!')]}},
        {'uuid': 'queued', 'type': 'attachment', 'attachment': {'type': 'queued_command', 'origin': {'kind': 'human'}, 'prompt': 'Then commit it.'}},
    ]


VISIBLE = ['human', 'bash', 'read', 'grep', 'answer:0', 'edit', 'bulletin:0', 'duration', 'human-2', 'build', 'check', 'queued']


def offscreen(view):
    """Every check window stays off screen and never becomes the active app (Alex, 2026-10-01 19:15 ET)."""
    assert view['window_on_screen'] is False and view['app_active'] is False, {k: view.get(k) for k in ('window_on_screen', 'app_active')}


def fresh_view(dump, after):
    """A .view.json written after `after`. An earlier timer tick can predate the finished read."""
    view_path = Path(str(dump) + '.view.json')
    found = {}

    def ready():
        try:
            st = view_path.stat()
        except OSError:
            return False
        if st.st_mtime <= after or st.st_size == 0:
            return False
        try:
            found['view'] = json.loads(view_path.read_text())
        except (OSError, ValueError):
            return False
        return 'transcript_bytes_read' in found['view']

    wait_for(ready, 8, .05, 'a fresh post-render view')
    return found['view']


def append(path, record):
    with path.open('a') as f:
        f.write(json.dumps(record) + '\n')


def snapshot(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return []


def wait_for(predicate, limit=20, step=.02, what='condition', detail=None):
    start = time.monotonic()
    while time.monotonic() - start < limit:
        value = predicate()
        if value:
            return time.monotonic() - start
        time.sleep(step)
    raise AssertionError(f'timed out after {limit} s waiting for {what}' + (f': {detail()}' if detail else ''))


def launch(file, dump, appearance='light', state='idle', mode='focus'):
    # Every check pins the mode, so it never touches the person's saved Focus | Full choice.
    return subprocess.Popen([str(BIN), '--transcript', str(file), '--dump-chat', str(dump), '--appearance', appearance,
                             '--chat-state', state, '--agent-name', 'Claude', '--chat-mode', mode],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def stop(process):
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def capture(dump, out):
    """--dump-chat writes <dump>.png of the chat window one second after launch."""
    source = Path(str(dump) + '.png')
    try:
        wait_for(source.exists, 8)
        time.sleep(.2)
        shutil.copyfile(source, out)
        return out.stat().st_size > 1000
    except (AssertionError, OSError):
        return False


def fixture_checks(temp, out_dir, lines):
    fixture = FIX / 'conversation.jsonl'
    fixture.write_text(''.join(json.dumps(r) + '\n' for r in records()))
    # Screenshots first, of the untouched fixture: light in Focus while working, dark in Full while blocked.
    for appearance, state, mode in (('light', 'working', 'focus'), ('dark', 'blocked', 'full')):
        dump = temp / f'dump-{appearance}.json'
        p = launch(fixture, dump, appearance, state, mode)
        try:
            wait_for(lambda: bool(snapshot(dump)))
            visible = snapshot(dump)
            ok = capture(dump, out_dir / f'P19-{appearance}.png')
            time.sleep(.3)
            view = json.loads(Path(str(dump) + '.view.json').read_text())
        finally:
            stop(p)
        offscreen(view)
        runs = {g['first']: (g['count'], g['tools'], g['open']) for g in view['tool_groups']}
        if mode == 'focus':
            assert runs == {'bash': (3, ['Bash', 'Read', 'Grep'], False), 'edit': (1, ['Edit'], False), 'build': (2, ['Bash', 'Bash'], False)}, runs
            assert view['activity'] == 'Run the P19 check', view['activity']
            lines.append('PASS --dump-chat view (focus, working): runs bash x3 [Bash, Read, Grep] and build x2 [Bash, Bash] folded; '
                         'live line "Run the P19 check"')
        else:
            assert all(o for c, _, o in runs.values() if c > 1), runs
            lines.append('PASS --dump-chat view (full): every run of tool calls open')
        lines.append(f'{"PASS" if ok else "FAIL"} fixture {appearance} screenshot ({mode}, {state} state): checks/P19-{appearance}.png')
        if not ok:
            raise AssertionError(f'{appearance} screenshot missing')
    ids = [r['id'] for r in visible]
    assert ids == VISIBLE, ids
    by = {r['id']: r for r in visible}
    assert by['human']['text'].startswith('The tool rows feel loud'), by['human']
    assert by['bash']['status'] == 'done' and by['bash']['result'].startswith('ChatView.swift')
    assert by['grep']['status'] == 'error' and by['grep']['result'] == 'boom'
    assert by['read']['status'] == 'done' and by['grep']['status'] != by['read']['status']
    assert by['edit']['status'] == 'done' and '-static let toolRow: CGFloat = 28' in by['edit']['input'] and '+.foregroundStyle(p.mute)' in by['edit']['input']
    assert by['check']['status'] == 'running' and by['build']['status'] == 'done'
    assert by['bulletin:0']['text'].startswith('from w5H:p6: P18 factory view landed')
    assert by['duration']['text'] == '1m 24s'
    assert by['queued']['queued'] is True and by['queued']['text'] == 'Then commit it.'
    assert '| Element | Before | After |' in by['answer:0']['text'] and '```swift' in by['answer:0']['text']
    assert 'HIDDEN' not in json.dumps(visible)
    lines.append(f'PASS fixture: exactly {len(VISIBLE)} visible items in order; sidechain, isMeta, compact summary, thinking and tool_result records hidden; '
                 'bash is done and grep is error (distinct); Edit diff, lane bulletin, queued prompt and turn duration associated')


def focus_checks(temp, lines):
    """Focus is the default: runs of tool calls fold to one line, a click opens one, the live line names the newest running tool."""
    fixture = FIX / 'conversation.jsonl'
    app = App(['--transcript', str(fixture), '--chat-state', 'working', '--agent-name', 'Claude', '--chat-mode', 'focus'],
              dict(os.environ), temp, 'focus.fifo')
    try:
        wait_for(lambda: len(app.state()['items']) == len(VISIBLE), 10, .1)
        time.sleep(.5)
        s = app.state()
        runs = {g['first']: g for g in s['tool_groups']}
        assert s['mode'] == 'focus', s['mode']
        assert {k: (g['count'], g['open']) for k, g in runs.items()} == {'bash': (3, False), 'edit': (1, False), 'build': (2, False)}, runs
        folded = {'bash', 'read', 'grep', 'build', 'check'}
        assert not folded & set(s['rendered_tools']), s['rendered_tools']
        assert s['activity'] == 'Run the P19 check', s['activity']
        lines.append('PASS focus (pinned): runs of 3 and 2 tool calls are one line each, none of their rows drawn; '
                     'the live line reads "Run the P19 check", the newest running tool')
        app.cmd({'cmd': 'click', 'target': 'group', 'id': 'build'})
        wait_for(lambda: {'build', 'check'} <= set(app.state()['rendered_tools']), 5, .1, 'the clicked run to draw its rows',
                 lambda: json.dumps(app.state()['rendered_tools']))
        s = app.state()
        assert next(g for g in s['tool_groups'] if g['first'] == 'build')['open'], s['tool_groups']
        assert next(g for g in s['tool_groups'] if g['first'] == 'bash')['open'] is False, s['tool_groups']
        lines.append('PASS focus click: the "2 tool calls" line opened to its Build and Check rows; the other run stayed folded')
        app.cmd({'cmd': 'mode', 'mode': 'full'})
        wait_for(lambda: app.state()['mode'] == 'full', 5, .1)
        assert all(g['open'] for g in app.state()['tool_groups'] if g['count'] > 1), app.state()['tool_groups']
        lines.append('PASS Full: every run opens')
    finally:
        app.stop()
    # Fresh defaults, no --chat-mode: the initial mode is Focus. Restore whatever was saved.
    domain, key = 'HerdrShell', 'HerdrShell.chatMode'
    before = subprocess.run(['defaults', 'read', domain, key], capture_output=True, text=True)
    subprocess.run(['defaults', 'delete', domain, key], capture_output=True)
    app = App(['--transcript', str(fixture), '--chat-state', 'idle'], dict(os.environ), temp, 'fresh.fifo')
    try:
        wait_for(lambda: bool(app.state()['items']), 10, .1)
        fresh = app.state()['mode']
        assert fresh == 'focus', fresh
    finally:
        app.stop()
        if before.returncode == 0:
            subprocess.run(['defaults', 'write', domain, key, before.stdout.strip()], check=True)
        else:
            subprocess.run(['defaults', 'delete', domain, key], capture_output=True)
    lines.append('PASS focus default: one launch with no --chat-mode and HerdrShell.chatMode deleted; initial mode is focus')
    # The choice persists in UserDefaults when no flag pins it. Restore whatever was saved before.
    domain, key = 'HerdrShell', 'HerdrShell.chatMode'
    before = subprocess.run(['defaults', 'read', domain, key], capture_output=True, text=True)
    app = App(['--transcript', str(fixture), '--chat-state', 'idle'], dict(os.environ), temp, 'persist.fifo')
    try:
        wait_for(lambda: bool(app.state()['items']), 10, .1)
        start_mode = app.state()['mode']
        target = 'full' if start_mode == 'focus' else 'focus'
        app.cmd({'cmd': 'mode', 'mode': target})
        wait_for(lambda: subprocess.run(['defaults', 'read', domain, key], capture_output=True, text=True).stdout.strip() == target, 5, .1,
                 'the mode in UserDefaults')
    finally:
        app.stop()
        if before.returncode == 0:
            subprocess.run(['defaults', 'write', domain, key, before.stdout.strip()], check=True)
        else:
            subprocess.run(['defaults', 'delete', domain, key], capture_output=True)
    lines.append(f'PASS mode persists: unpinned window started in {start_mode} (saved value: {before.stdout.strip() or "none, so Focus"}); '
                 f'switching to {target} wrote {domain} {key}={target}; the saved value was restored afterwards')


def append_checks(temp, lines):
    file = temp / 'append.jsonl'
    file.write_bytes((FIX / 'conversation.jsonl').read_bytes())
    dump = temp / 'append.json'
    p = launch(file, dump)
    try:
        wait_for(lambda: bool(snapshot(dump)))
        latencies = []
        for i in range(20):
            item = {'uuid': f'append-{i}', 'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': f'Update {i}'}]}}
            start = time.monotonic()
            append(file, item)
            wait_for(lambda: any(r['id'] == f'append-{i}:0' for r in snapshot(dump)), 1.5, .005)
            latencies.append((time.monotonic() - start) * 1000)
    finally:
        stop(p)
    ordered = sorted(latencies)
    lines.append(f'PASS append n=20: every message within 1.5 s; p50={statistics.median(latencies):.1f} ms p95={ordered[18]:.1f} ms max={max(latencies):.1f} ms')


def large_checks(temp, lines):
    large = temp / 'large.jsonl'
    hidden = (json.dumps({'type': 'unknown', 'padding': 'x' * 1000}) + '\n').encode()
    with large.open('wb') as f:
        for _ in range((300 * 1024 * 1024) // len(hidden) + 1):
            f.write(hidden)
        f.write((json.dumps({'uuid': 'large-visible', 'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'Large transcript ready'}]}}) + '\n').encode())
    dump = temp / 'large.json'
    start = time.monotonic()
    p = launch(large, dump)
    rss_max = 0
    try:
        while not snapshot(dump) and time.monotonic() - start < 30:
            rss = subprocess.run(['ps', '-o', 'rss=', '-p', str(p.pid)], capture_output=True, text=True).stdout.strip()
            rss_max = max(rss_max, int(rss or 0))
            time.sleep(.02)
        elapsed = (time.monotonic() - start) * 1000
        assert snapshot(dump)[-1]['id'] == 'large-visible:0'
        assert rss_max < 300 * 1024, rss_max
        view = fresh_view(dump, time.time())
        offscreen(view)
        read_n = view['transcript_bytes_read']
        file_n = large.stat().st_size
        slack = 1024 * 1024
        assert 2 * 1024 * 1024 <= read_n <= 2 * 1024 * 1024 + slack, read_n
        assert read_n < file_n // 2, (read_n, file_n)
    finally:
        stop(p)
        large.unlink(missing_ok=True)
    lines.append(f'PASS 300 MiB transcript: first render {elapsed:.0f} ms, peak sampled RSS {rss_max / 1024:.1f} MiB (< 300 MiB); '
                 f'transcript bytes read {read_n} (≤ 2 MiB + 1 MiB slack, file was {file_n} bytes)')


RECORD_96 = {
    'old': b'{"uuid":"old","type":"assistant","message":{"content":[{"type":"text","text":"old"}]},"pad":""}\n',
    'new': b'{"uuid":"new","type":"assistant","message":{"content":[{"type":"text","text":"new"}]},"pad":""}\n',
}
assert len(RECORD_96['old']) == len(RECORD_96['new']) == 96


def write_fake(path, log, screen):
    path.write_text(f'''#!/usr/bin/env python3
import json, os, sys, time
args = sys.argv[1:]
log = {str(log)!r}
screen = {screen!r}
if args[:2] == ["agent", "get"]:
    print(json.dumps({{"result": {{"agent": {{"agent": "claude", "agent_session": {{"value": "sess"}}, "agent_status": "idle", "cwd": "/tmp", "terminal_title_stripped": "Claude"}}}}}}))
elif len(args) >= 2 and args[0] == "pane" and args[1] == "send-text":
    chunk = args[-1]
    mark = "R" if chunk == "\\r" else (chunk[0] if chunk else "?")
    with open(log, "a") as handle:
        handle.write(f"{{time.time():.6f}} {{mark}}\\n")
    if os.environ.get("P19_SEND_DELAY"):
        time.sleep(float(os.environ["P19_SEND_DELAY"]))
    print("ok")
elif args[:2] == ["pane", "read"]:
    sys.stdout.write(screen)
else:
    print("ok")
''')
    os.chmod(path, 0o755)


def draft_unknown_check(temp, lines):
    """No ❯ in the last 12 rows: hold and warn, do not send."""
    log = temp / 'draft-sends.log'
    fake = temp / 'fake-herdr'
    write_fake(fake, log, 'scrolled away from the prompt\nstill no marker on this row\n')
    app = App(['--demo', 'chat', '--pane', 'p19-draft-probe', '--herdr', str(fake), '--chat-mode', 'focus'],
              dict(os.environ), temp, 'draft.fifo')
    try:
        wait_for(lambda: app.state().get('composer_focused'), 10, .1)
        app.type('hello unknown')
        time.sleep(.3)
        app.key('return')
        wait_for(lambda: app.state().get('warning') and app.state().get('status') == "can't see the prompt; send anyway?",
                 8, .1, 'the unknown-prompt warning', lambda: json.dumps({k: app.state().get(k) for k in ('status', 'warning', 'pending')}))
        time.sleep(.8)
        sends = log.read_text() if log.exists() else ''
        assert 'hello unknown' not in sends and not log.exists(), sends
        s = app.state()
        assert s['pending'] == 'hello unknown' and s['warning'] is True, s
    finally:
        app.stop()
    lines.append('PASS draft unknown: last 12 rows had no ❯; Enter held "hello unknown" and warned '
                 '"can\'t see the prompt; send anyway?"; send-text was not called')


def rewrite_check(temp, lines):
    """Truncate and rewrite an equal-length record. The dump must show `new`, not `old`."""
    file = temp / 'rewrite.jsonl'
    dump = temp / 'rewrite.json'
    file.write_bytes(RECORD_96['old'])
    p = launch(file, dump)
    try:
        wait_for(lambda: any(i.get('text') == 'old' for i in snapshot(dump)), 8, .05, 'the old record')
        fd = os.open(file, os.O_RDWR)
        os.ftruncate(fd, 0)
        os.write(fd, RECORD_96['new'])
        os.close(fd)
        assert file.stat().st_size == 96
        wait_for(lambda: any(i.get('text') == 'new' for i in snapshot(dump)) and not any(i.get('text') == 'old' for i in snapshot(dump)),
                 5, .05, 'the rewritten record', lambda: json.dumps(snapshot(dump)))
        view_path = Path(str(dump) + '.view.json')
        wait_for(lambda: view_path.exists() and view_path.stat().st_size > 0, 8)
        offscreen(json.loads(view_path.read_text()))
    finally:
        stop(p)
    lines.append('PASS equal-length rewrite: 96-byte record `old` truncated and replaced with `new`; dump shows new')


def pad_record(size=8192):
    prefix, suffix = '{"type":"unknown","padding":"', '"}\n'
    fill = size - len(prefix) - len(suffix)
    assert fill > 0
    return (prefix + ('x' * fill) + suffix).encode()


def rewrite_middle_check(temp, lines):
    """Same inode and size: only the middle 96 bytes change. Head and tail 4 KiB stay put, so a head/tail sample misses it."""
    pad = pad_record()
    assert len(pad) == 8192
    file = temp / 'rewrite-middle.jsonl'
    body = pad + RECORD_96['old'] + pad
    file.write_bytes(body)
    app = App(['--transcript', str(file), '--chat-mode', 'focus', '--chat-state', 'idle'], dict(os.environ), temp, 'rewrite-middle.fifo')
    try:
        def texts():
            return [i.get('text') for i in app.state()['items']]
        wait_for(lambda: texts() == ['old'], 8, .05, 'the middle old record', lambda: str(texts()))
        ino = file.stat().st_ino
        mtime = file.stat().st_mtime
        os.kill(app.p.pid, signal.SIGSTOP)
        wait_for(lambda: subprocess.run(['ps', '-o', 'state=', '-p', str(app.p.pid)], capture_output=True, text=True).stdout.strip().startswith('T'),
                 2, .02, 'the app to stop')
        rewritten = pad + RECORD_96['new'] + pad
        assert len(rewritten) == len(body)
        assert rewritten[:4096] == body[:4096] and rewritten[-4096:] == body[-4096:]
        assert rewritten[8192:8192 + 96] == RECORD_96['new']
        fd = os.open(file, os.O_RDWR)
        os.ftruncate(fd, 0)
        os.write(fd, rewritten)
        os.close(fd)
        assert file.stat().st_ino == ino and file.stat().st_size == len(body)
        ahead = time.time() + 5
        os.utime(file, (ahead, ahead))
        assert file.stat().st_mtime > mtime
        os.kill(app.p.pid, signal.SIGCONT)
        wait_for(lambda: texts() == ['new'], 5, .05, 'the rewritten middle record', lambda: str(texts()))
    finally:
        if app.p.poll() is None:
            try:
                os.kill(app.p.pid, signal.SIGCONT)
            except ProcessLookupError:
                pass
        app.stop()
    lines.append('PASS middle rewrite: same inode, two 8 KiB pads around a 96-byte record; SIGSTOP, replace old with new, '
                 'mtime forward, SIGCONT; a fresh state shows new')


def earlier_check(temp, lines):
    """A tail of 400 visible items must not discard the block Load earlier reads."""
    path = temp / 'earlier.jsonl'
    earlier = (json.dumps({'uuid': 'earlier-marker', 'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'EARLIER-VISIBLE'}]}}) + '\n').encode()
    pad = (json.dumps({'type': 'unknown', 'padding': 'x' * 180}) + '\n').encode()
    tail = b''.join(
        (json.dumps({'uuid': f'tail-{i}', 'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': f'tail {i}'}]}}) + '\n').encode()
        for i in range(400)
    )
    with path.open('wb') as handle:
        handle.write(earlier)
        while handle.tell() < 2 * 1024 * 1024 + len(earlier):
            handle.write(pad)
        handle.write(tail)
    assert path.stat().st_size > 2 * 1024 * 1024
    app = App(['--transcript', str(path), '--chat-mode', 'focus', '--chat-state', 'idle'], dict(os.environ), temp, 'earlier.fifo')
    try:
        def ids():
            return [i['id'] for i in app.state()['items']]
        wait_for(lambda: len(ids()) >= 400 and 'earlier-marker:0' not in ids(), 20, .1, 'the 400-item tail', lambda: str(len(ids())))
        assert app.state()['earlier'] is True
        app.cmd({'cmd': 'load_earlier'})
        wait_for(lambda: 'earlier-marker:0' in ids(), 15, .1, 'the earlier item to stay visible', lambda: str(ids()[:3]))
        assert any(i['text'] == 'EARLIER-VISIBLE' for i in app.state()['items'])
    finally:
        app.stop()
    lines.append('PASS load earlier: file over 2 MiB with 400 tail items; after Load earlier, EARLIER-VISIBLE is in the list')


def race_check(temp, lines):
    """Two 600-scalar sends to one pane must not interleave their chunks."""
    log = temp / 'race.log'
    fake = temp / 'fake-herdr-race'
    write_fake(fake, log, '\n❯ \n')
    env = dict(os.environ, P19_SEND_DELAY='0.15')
    app = App(['--demo', 'chat', '--pane', 'p19-race', '--herdr', str(fake), '--chat-mode', 'focus'], env, temp, 'race.fifo')
    try:
        wait_for(lambda: app.state().get('composer_focused'), 10, .1)
        app.cmd({'cmd': 'race'})
        def marks():
            if not log.exists():
                return []
            rows = []
            for line in log.read_text().splitlines():
                t, m = line.split()
                rows.append((float(t), m))
            return rows
        wait_for(lambda: len(marks()) >= 6, 15, .1, 'six send-text calls', lambda: str(marks()))
        events = marks()
        letters = ''.join(m for _, m in events)
        r_times = [t for t, m in events if m == 'R']
        a_times = [t for t, m in events if m == 'A']
        b_times = [t for t, m in events if m == 'B']
        assert len(a_times) == 2 and len(b_times) == 2 and len(r_times) == 2, letters
        first_r = min(r_times)
        assert min(a_times) > first_r or min(b_times) > first_r, letters
        assert letters in ('AARBBR', 'BBRAAR'), letters
    finally:
        app.stop()
    lines.append(f'PASS per-pane send queue: two 600-scalar sends logged {letters} (one message finished before the other started)')


def race_readonly_check(temp, lines):
    """The race hook's second sender must inherit --read-only and deliver nothing."""
    log = temp / 'race-ro-sends.log'
    err = temp / 'race-ro.err'
    fake = temp / 'fake-herdr-race-ro'
    write_fake(fake, log, '\n❯ \n')
    env = dict(os.environ, P19_SEND_DELAY='0.05')
    app = App(['--demo', 'chat', '--pane', 'P', '--read-only', '--herdr', str(fake), '--chat-mode', 'focus'],
              env, temp, 'race-ro.fifo', log=str(err))
    try:
        wait_for(lambda: app.state().get('composer_focused'), 10, .1)
        app.cmd({'cmd': 'race'})
        time.sleep(1)
        sends = log.read_text() if log.exists() else ''
        assert sends == '', sends
        err_text = err.read_text() if err.exists() else ''
        assert 'Read-only: this window does not send' in err_text, err_text
        status = app.state()['status']
        assert status == 'Read-only: this window does not send', status
    finally:
        app.stop()
    lines.append('PASS read-only race: --demo chat --pane P --read-only --control, {"cmd":"race"} delivered nothing '
                 'and logged "Read-only: this window does not send"')


def combo_check(temp, lines):
    """--transcript and --pane together must not send unless --read-only."""
    fixture = FIX / 'conversation.jsonl'
    if not fixture.exists():
        fixture.write_text(''.join(json.dumps(r) + '\n' for r in records()))
    fake = temp / 'fake-herdr-combo'
    write_fake(fake, temp / 'combo.log', '\n❯ \n')
    proc = subprocess.Popen([str(BIN), '--transcript', str(fixture), '--pane', 'p19-combo', '--herdr', str(fake)],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        _, stderr = proc.communicate(timeout=3)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
        raise AssertionError('--transcript with --pane kept running without --read-only')
    text = stderr.decode()
    assert proc.returncode == 2, (proc.returncode, text)
    assert 'read-only' in text, text
    dump = temp / 'combo.json'
    allowed = subprocess.Popen([str(BIN), '--transcript', str(fixture), '--pane', 'p19-combo', '--read-only',
                                '--herdr', str(fake), '--dump-chat', str(dump)],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(.4)
        assert allowed.poll() is None, allowed.returncode
    finally:
        stop(allowed)
    lines.append(f'PASS reject --transcript with --pane unless --read-only: exit 2; {text.strip()}')


# ---- lab ----

def lab(*args):
    return subprocess.run([sys.executable, str(ROOT / 'scripts/lab.py'), *args], capture_output=True, text=True).stdout


def lab_json(*args):
    return json.loads(lab('herdr', *args))


def lab_env():
    env = dict(line.split('=', 1) for line in lab('env').splitlines() if '=' in line)
    assert env['HERDR_SOCKET_PATH'].startswith(str(LAB) + os.sep), env['HERDR_SOCKET_PATH']
    return env


def agent(pane):
    try:
        return lab_json('agent', 'get', pane)['result']['agent']
    except (ValueError, KeyError):
        return {}


def screen(pane):
    return lab('herdr', 'pane', 'read', pane, '--source', 'recent', '--lines', '40')


class App:
    """A chat window driven through its control FIFO."""

    def __init__(self, argv, env, temp, fifo_name, log=os.devnull):
        self.fifo = str(temp / fifo_name)
        self.state_path = temp / (fifo_name + '-state.json')
        if os.path.exists(self.fifo):
            os.unlink(self.fifo)
        self.p = subprocess.Popen([str(BIN), *argv, '--control', self.fifo], env=env, stdout=subprocess.DEVNULL, stderr=open(log, 'a'))
        wait_for(lambda: os.path.exists(self.fifo), 10)

    def cmd(self, obj):
        # The hook reads to EOF and reopens; a writer that opened just as the previous
        # reader closed gets EPIPE with nothing consumed, so writing again is safe.
        for _ in range(50):
            try:
                with open(self.fifo, 'w') as f:
                    f.write(json.dumps(obj) + '\n')
                return
            except BrokenPipeError:
                time.sleep(.05)
        raise BrokenPipeError(self.fifo)

    def type(self, text):
        self.cmd({'cmd': 'type', 'text': text})

    def key(self, key, mods=()):
        self.cmd({'cmd': 'key', 'key': key, 'mods': list(mods)})

    def state(self):
        self.state_path.unlink(missing_ok=True)
        self.cmd({'cmd': 'state', 'out': str(self.state_path)})
        wait_for(lambda: self.state_path.exists() and self.state_path.stat().st_size > 0, 5)
        state = json.loads(self.state_path.read_text())
        offscreen(state)
        return state

    def shot(self, out):
        self.cmd({'cmd': 'shot', 'out': str(out)})
        wait_for(lambda: out.exists() and out.stat().st_size > 1000, 5)

    def stop(self):
        stop(self.p)


def lab_checks(temp, out_dir, lines):
    lab('down')
    time.sleep(.5)
    lab('up')
    env = lab_env()
    pane = lab_json('tab', 'create', '--workspace', 'w1', '--label', 'fake claude', '--cwd', '/tmp', '--no-focus')['result']['root_pane']['pane_id']
    wait_for(lambda: '%' in lab('herdr', 'pane', 'read', pane, '--source', 'visible'), 10, .05)
    session = 'p19-' + uuid.uuid4().hex[:8]
    claude_home = Path(env['HOME']) / '.claude'
    cwd = agent(pane).get('cwd') or lab_json('pane', 'get', pane)['result']['pane']['cwd']
    encoded = ''.join(c if c.isascii() and c.isalnum() else '-' for c in cwd)
    transcript = claude_home / 'projects' / encoded / f'{session}.jsonl'
    transcript.parent.mkdir(parents=True, exist_ok=True)
    transcript.write_text(json.dumps({'uuid': 'hello', 'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'Lab session ready. Type below.'}]}}) + '\n')
    fake = LAB / 'fake' / 'claude'
    fake.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / 'scripts/p19_fake_claude.py', fake)
    python = os.path.realpath(sys.executable)
    lab('herdr', 'pane', 'run', pane, f'{python} {fake} {transcript}')
    wait_for(lambda: agent(pane).get('agent') == 'claude', 10, .1)
    lab('herdr', 'pane', 'report-agent-session', pane, '--source', 'herdr:claude', '--agent', 'claude', '--agent-session-id', session)
    wait_for(lambda: (agent(pane).get('agent_session') or {}).get('value') == session, 5, .1)
    lines.append(f'lab {LAB_NAME}: pane {pane} runs a script named claude (herdr detects agent=claude by process); '
                 f'native session report {session}; transcript {transcript.relative_to(LAB)}')
    env = dict(env, CLAUDE_HOME=str(claude_home))
    app = App(['--demo', 'chat', '--pane', pane, '--herdr', str(LAB / 'bin/herdr'), '--socket', env['HERDR_SOCKET_PATH'],
               '--appearance', 'light', '--chat-mode', 'focus'], env, temp, 'chat.fifo', LAB / 'chat-app.log')
    try:
        wait_for(lambda: any(i['id'] == 'hello:0' for i in app.state()['items']), 10, .1)
        s = app.state()
        assert s['composer_focused'], s
        lines.append(f'PASS lab chat opened the fake session from `herdr agent get` (state {s["agent_state"]}); composer is first responder')

        # Shift+Enter is a newline and sends nothing.
        app.type('a'); app.key('return', ['shift']); app.type('b')
        time.sleep(.3)
        s = app.state()
        assert s['composer_text'] == 'a\nb' and not s['pending'], s
        for _ in range(3):
            app.key('backspace')
        time.sleep(.2)
        assert app.state()['composer_text'] == ''
        lines.append('PASS Shift+Enter inserts a newline in the composer and sends nothing')

        # 1. Send: real key events, then Enter.
        message = 'hello from the composer'
        start = time.monotonic()
        app.type(message); app.key('return')
        wait_for(lambda: f'received: {message}' in screen(pane), 10, .05)
        arrived = time.monotonic() - start
        wait_for(lambda: any(i['kind'] == 'user' and i['text'] == message for i in app.state()['items']), 10, .1)
        s = app.state()
        assert s['composer_text'] == '' and s['pending'] == '', s
        lines.append(f'PASS send: typed "{message}" + Enter in the composer; `herdr pane read` shows "received: {message}" '
                     f'(the fake reads a whole line, so the text and its trailing return arrived) {arrived:.2f} s after the last key; '
                     'the You message came back from the transcript and the sending state cleared')

        # 2. Draft in the pane: warning, nothing sent.
        lab('herdr', 'pane', 'send-text', pane, 'half typed in the terminal')
        wait_for(lambda: '❯ half typed in the terminal' in screen(pane), 5, .05)
        draft = 'second message'
        app.type(draft); app.key('return')
        wait_for(lambda: app.state()['warning'], 15, .1, 'the draft warning', lambda: json.dumps({k: v for k, v in app.state().items() if k != 'items'}) + ' screen: ' + screen(pane)[-200:])
        time.sleep(2)
        s = app.state()
        assert s['status'] == "There's unsent text in the terminal" and s['pending'] == draft, s
        assert f'received: {draft}' not in screen(pane) and draft not in screen(pane), screen(pane)
        assert not any(i['kind'] == 'user' and i['text'] == draft for i in s['items'])
        app.shot(out_dir / 'P19-draft.png')
        lines.append('PASS draft: with "half typed in the terminal" after the pane\'s ❯, Enter showed "There\'s unsent text in the terminal" '
                     '(Send anyway / Cancel); 2 s later the pane and transcript show nothing from the composer (checks/P19-draft.png)')
        app.cmd({'cmd': 'click', 'target': 'cancel'})
        lab('herdr', 'pane', 'send-text', '--human', pane, '\r')  # Enter in the terminal: the lab's own draft goes through
        wait_for(lambda: 'received: half typed in the terminal' in screen(pane), 8, .05, 'the lab draft to go through', detail=lambda: screen(pane)[-300:])
        assert app.state()['pending'] == ''

        # 3. Blocked: held, then sent once the dialog closes.
        lab('herdr', 'pane', 'send-text', pane, '__block__\r')
        wait_for(lambda: agent(pane).get('agent_status') == 'blocked', 10, .1, 'herdr to report blocked', detail=lambda: screen(pane)[-300:])
        held = 'third message'
        app.type(held); app.key('return')
        wait_for(lambda: app.state()['status'].startswith('held'), 15, .1, 'held status', lambda: app.state()['status'])
        wait_for(lambda: app.state()['agent_state'] == 'blocked', 5, .1)
        time.sleep(4)
        assert f'received: {held}' not in screen(pane) and held not in screen(pane), screen(pane)
        app.shot(out_dir / 'P19-held.png')
        lines.append('PASS blocked: herdr reported the fake\'s permission dialog as blocked; Enter held "third message" '
                     '("held: will send when the terminal stops asking"), nothing reached the pane in 4 s (checks/P19-held.png)')
        lab('herdr', 'pane', 'send-text', pane, '__unblock__\r')
        unblocked = time.monotonic()
        wait_for(lambda: agent(pane).get('agent_status') != 'blocked', 10, .1, 'herdr to clear blocked')
        wait_for(lambda: f'received: {held}' in screen(pane), 10, .05, 'the held message', detail=lambda: screen(pane)[-300:])
        lines.append(f'PASS unblocked: the held message reached the pane {time.monotonic() - unblocked:.1f} s after the dialog closed (3 s recheck)')
    finally:
        app.stop()
        lab('down')


def live_check(pane, temp, out_dir, lines):
    info = json.loads(subprocess.run(['herdr', 'agent', 'get', pane], capture_output=True, text=True).stdout)['result']['agent']
    assert info.get('agent') == 'claude' and (info.get('agent_session') or {}).get('value'), info
    fifo = temp / 'live.fifo'
    state = temp / 'live-state.json'
    p = subprocess.Popen([str(BIN), '--demo', 'chat', '--pane', pane, '--read-only', '--chat-mode', 'focus', '--control', str(fifo)],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_for(fifo.exists, 10)

        def loaded():
            state.unlink(missing_ok=True)
            with open(fifo, 'w') as f:
                f.write(json.dumps({'cmd': 'state', 'out': str(state)}) + '\n')
            time.sleep(.2)
            return state.exists() and len(json.loads(state.read_text())['items']) > 3
        wait_for(loaded, 20, .3)
        time.sleep(1)
        out = out_dir / 'P19-live.png'
        out.unlink(missing_ok=True)
        with open(fifo, 'w') as f:
            f.write(json.dumps({'cmd': 'shot', 'out': str(out)}) + '\n')
        wait_for(lambda: out.exists() and out.stat().st_size > 1000, 5)
        s = json.loads(state.read_text())
        offscreen(s)
    finally:
        stop(p)
    lines.append(f'PASS live: --demo chat --pane {pane} --read-only rendered {len(s["items"])} items from the real session '
                 f'(agent state {s["agent_state"]}); no key events and no sends (checks/P19-live.png)')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', default='checks/P19.txt')
    parser.add_argument('--live-pane', help='a real Claude pane to render read-only for checks/P19-live.png')
    parser.add_argument('--skip-lab', action='store_true')
    parser.add_argument('--only', action='append', default=[], help='run just these step names')
    args = parser.parse_args()
    out = ROOT / args.out
    out.parent.mkdir(parents=True, exist_ok=True)
    FIX.mkdir(parents=True, exist_ok=True)
    lines = [f'P19 chat view check  {time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}  load {os.getloadavg()[0]:.0f} on {os.cpu_count()} cores']
    failure = False
    steps = [('fixture', lambda t: fixture_checks(t, out.parent, lines)), ('focus', lambda t: focus_checks(t, lines)),
             ('append', lambda t: append_checks(t, lines)),
             ('large transcript', lambda t: large_checks(t, lines)),
             ('draft unknown', lambda t: draft_unknown_check(t, lines)),
             ('rewrite', lambda t: rewrite_check(t, lines)),
             ('rewrite middle', lambda t: rewrite_middle_check(t, lines)),
             ('load earlier', lambda t: earlier_check(t, lines)),
             ('send queue', lambda t: race_check(t, lines)),
             ('read-only race', lambda t: race_readonly_check(t, lines)),
             ('transcript pane', lambda t: combo_check(t, lines))]
    if not args.skip_lab and not args.only:
        steps.append(('lab', lambda t: lab_checks(t, out.parent, lines)))
    if args.only:
        steps = [step for step in steps if step[0] in args.only]
    if args.live_pane:
        steps.append(('live', lambda t: live_check(args.live_pane, t, out.parent, lines)))
    with tempfile.TemporaryDirectory(prefix='p19-', dir=os.environ.get('TMPDIR')) as temp:
        for name, step in steps:
            try:
                step(Path(temp))
            except Exception as error:
                failure = True
                lines.append(f'FAIL {name}: {type(error).__name__} {error}')
    if args.skip_lab:
        lines.append('NOT RUN lab: --skip-lab')
    if not args.live_pane:
        lines.append('NOT RUN live screenshot: no --live-pane given')
    out.write_text('\n'.join(lines) + '\n')
    print(out.read_text(), end='')
    return 1 if failure else 0


if __name__ == '__main__':
    raise SystemExit(main())
