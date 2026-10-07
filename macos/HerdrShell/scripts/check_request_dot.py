#!/usr/bin/env python3
"""Pure compiled SpacesTree check: Agents-only requests, never app/server access.

The row boundary guards request presence, clear, ordinary pin and non-agent role, and
that a face folds the request into its own dot so a row shows at most one blue dot;
existing machine checks do not exercise request metadata. No production test seam.
"""
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="herdr-request-dot-", dir=os.environ.get("TMPDIR")) as scratch:
    build = pathlib.Path(scratch)
    source = build / "request_dot.swift"
    source.write_text(r'''
import Foundation
@main struct Check {
    static func main() {
        var count = 0
        for (role, request, expected) in [("agent", "req-42", true), ("agent", "", false), ("", "req-42", false), ("other", "req-42", false)] {
            let tab = SpacesInput.Tab(id: "tab", space: "ws", label: "chat", pinIndex: 0,
                                      role: role.isEmpty ? nil : role, request: request.isEmpty ? nil : request)
            let input = SpacesInput(spaces: [SpacesInput.Space(id: "ws", name: "space")], tabs: [tab])
            let rows = SpacesTree.build(input, overlay: Overlay(), chrome: SpacesChrome(), now: 0)
            let dots = rows.filter { $0.request != nil }
            precondition(dots.count == (expected ? 1 : 0), "only agent rows with a token get a dot")
            if expected {
                precondition(dots[0].id == "agent:tab" && dots[0].request == "req-42", "tooltip request id")
                // One blue dot per row: the face carries the request ahead of any state, so no
                // trailing dot is drawn beside it. A row without a face keeps the trailing dot.
                var row = dots[0]
                precondition(row.face != nil && row.trailingRequest == nil, "a face takes the request dot")
                for tone in ["working", "blocked", "done", "mute"] {
                    row.tone = tone
                    precondition(row.faceDot == "accent", "request beats \(tone)")
                }
                row.request = nil
                row.tone = "working"; precondition(row.faceDot == "ok", "working without a request is ok")
                row.tone = "mute"; precondition(row.faceDot == nil, "idle shows no dot")
                row.request = "req-42"; row.face = nil
                precondition(row.trailingRequest == "req-42" && row.faceDot == nil, "no face keeps the trailing dot")
            } else {
                precondition(rows.allSatisfy { $0.trailingRequest == nil }, "no request, no trailing dot")
            }
            count += 1
        }
        print("PASS request_dot: \(count) cases (present, absent/clear, ordinary pin, non-agent role); one blue dot per row")
    }
}
''')
    driver = build / "request_dot"
    subprocess.run(["swiftc", str(ROOT / "Sources/HerdrShell/SpacesTree.swift"), str(source), "-o", str(driver)], check=True, timeout=240)
    try:
        subprocess.run([str(driver)], check=True, timeout=60)
    except subprocess.TimeoutExpired:
        raise SystemExit("BLOCKED: compiled request_dot driver hung at launch over 60 s; not retried")
