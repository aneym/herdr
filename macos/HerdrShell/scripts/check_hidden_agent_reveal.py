#!/usr/bin/env python3
"""Golden regression at the persisted chrome / Swift subprocess boundary.

Selecting a hidden agent must open its fold without opening its space even when
no hiddenagent row is drawn. The lead-owned scenario selects an already drawn
hidden row, so it misses this regression. Uses snapshot-driven reveal input and
real Codable persistence, no mocks or test-only production seam. Runs through
the Swift interpreter, never a new binary. Not run during the load-bound fix.
"""
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
DRIVER = r'''
let cases: [(String, Bool, Bool, Bool, String)] = [
    // name, hidden agent, agent fold open, space folded, expected saved result
    ("shut/folded", true, false, true, "true|true|true"),
    ("shut/unfolded", true, false, false, "true|true|false"),
    ("open/folded", true, true, true, "false|true|true"),
    ("ordinary/folded", false, false, true, "true|false|false"),
]
for (name, hidden, open, folded, expected) in cases {
    var chrome = SpacesChrome()
    chrome.hiddenAgentsExpanded = open
    if folded { chrome.collapsedSpaces.insert("s1") }
    // A shut Hidden section has no hiddenagent rows to consult.
    let changed = chrome.reveal(space: "s1", parked: false, selected: "s1:t2",
                                rows: [], hiddenAgent: hidden)
    let saved = try JSONDecoder().decode(SpacesChrome.self, from: JSONEncoder().encode(chrome))
    let result = "\(changed)|\(saved.hiddenAgentsExpanded)|\(saved.collapsedSpaces.contains("s1"))"
    guard result == expected else {
        FileHandle.standardError.write(Data(("FAIL \(name): \(result), expected \(expected)\n").utf8))
        exit(1)
    }
    print("PASS " + name)
}
print("4 hidden-agent reveal regressions passed")
'''
with tempfile.TemporaryDirectory(prefix='hidden-agent-reveal-') as tmp:
    scratch = pathlib.Path(tmp)
    source = scratch / 'reveal.swift'
    source.write_text((ROOT / 'Sources/HerdrShell/SpacesTree.swift').read_text() + '\n' + DRIVER)
    subprocess.run(['swift', '-module-cache-path', str(scratch / 'modules'), str(source)], check=True)
