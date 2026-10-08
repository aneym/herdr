"""S5a decision contract and real-file persistence, without network or SSH.

The requested pure decision unit table guards precedence and stale-lock boundary
cases. No existing fanout coverage owns these cases; neither test needs a
production-only seam. The file integration test catches lost rollback/error
state and partial JSON writes at the persistence boundary.
"""

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
# Isolated Python omits the script directory; load the local stdlib-only modules.
for name in ('pc', 'fanout'):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
fanout = sys.modules['fanout']


class FanoutTests(unittest.TestCase):
    def test_decision_contract(self):
        cases = [
            ({}, 'new', False, None, 'build'),
            ({'built': 'old'}, 'new', False, None, 'build'),
            ({'built': 'new'}, 'new', False, None, 'skip'),
            ({'built': 'new'}, 'new', True, None, 'skip'),
            ({'built': 'old'}, 'new', True, None, 'wait'),
            ({}, 'new', False, 0, 'wait'),
            ({}, 'new', False, 3599.99, 'wait'),
            ({'built': 'new'}, 'new', False, 12, 'wait'),
            ({}, 'new', False, 3600, 'build'),
            ({}, 'new', False, 3601, 'build'),
            ({}, 'new', True, 3600, 'wait'),
            ({'built': 'new'}, 'new', False, 3600, 'skip'),
        ]
        for state, sha, game, age, expected in cases:
            with self.subTest(state=state, game=game, age=age):
                self.assertEqual(fanout.decide(state, sha, game, age), expected)

    def test_failed_fetch_backoff_table(self):
        # Pure retry schedule: 30 min, then 60 min, then never for that sha.
        f = lambda n, last: {'fetch_failures': {'s': {'n': n, 'last': last}}}
        cases = [
            ({}, 0, True),
            ({'fetch_failures': {'other': {'n': 9, 'last': 0}}}, 0, True),
            (f(1, 1000), 1000 + 1799, False),
            (f(1, 1000), 1000 + 1800, True),
            (f(2, 1000), 1000 + 3599, False),
            (f(2, 1000), 1000 + 3600, True),
            (f(3, 1000), 10 ** 9, False),
        ]
        for state, now, expected in cases:
            with self.subTest(state=state, now=now):
                self.assertEqual(fanout.fetch_due(state, 's', now), expected)

    def test_state_file_round_trip(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'nested/state.json'
            self.assertEqual(fanout.read_state(path),
                             {'built': None, 'staged_at': None, 'last_error': None})
            state = {'built': 'a' * 40, 'staged_at': '2026-10-06T12:00:00+00:00',
                     'last_error': None}
            fanout.write_state(state, path)
            self.assertEqual(json.loads(path.read_text()), state)
            state['last_error'] = 'PC staging failed (1)'
            fanout.write_state(state, path)
            self.assertEqual(fanout.read_state(path), state)
            self.assertFalse(path.with_suffix('.json.tmp').exists())


if __name__ == '__main__':
    unittest.main()
