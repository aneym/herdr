#!/usr/bin/env python3
"""Compiled check of the chat state colours both Shell sidebars draw: working is orange, an
unread finish is green, blocked stays red and idle stays grey (Alex, 2026-09-29: "an agent is
working, it should be oragne. finished, green ... the idle styling is fine"), and the row glyph
shapes the TUI draws (working a circle, done and blocked a square). Pure: compiles
Theme.swift, ShellTokens.swift and SpacesTree.swift, no app or server.
"""
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "Sources/HerdrShell"
with tempfile.TemporaryDirectory(prefix="herdr-state-colors-", dir=os.environ.get("TMPDIR")) as scratch:
    build = pathlib.Path(scratch)
    source = build / "state_colors.swift"
    source.write_text(r'''
import Foundation
func log(_ message: String) { FileHandle.standardError.write(Data((message + "\n").utf8)) }
/// Orange: red channel high, green between, blue low.
func orange(_ c: UInt32) -> Bool { let r = (c >> 16) & 0xFF, g = (c >> 8) & 0xFF, b = c & 0xFF; return r > g && g > b && Double(g) > Double(r) * 0.3 && Double(g) < Double(r) * 0.75 }
/// Green: green channel leads.
func green(_ c: UInt32) -> Bool { let r = (c >> 16) & 0xFF, g = (c >> 8) & 0xFF, b = c & 0xFF; return g > r && g > b }
@main struct Check {
    static func main() {
        for (mode, p) in [("dark", ChromePalette.dark), ("light", ChromePalette.light)] {
            let t = Tokens(mode: mode == "dark" ? .dark : .light, chrome: p, terminalBg: p.windowBg, terminalFg: p.ink)
            precondition(orange(t.stateTone("working")), "\(mode): working is orange, got \(String(t.stateTone("working"), radix: 16))")
            precondition(green(t.stateTone("done")), "\(mode): an unread finish is green, got \(String(t.stateTone("done"), radix: 16))")
            precondition(t.stateTone("blocked") == p.bad, "\(mode): blocked stays bad")
            precondition(t.stateTone("idle") == p.mute && t.stateTone("mute") == p.mute, "\(mode): idle keeps its grey")
            precondition(p.value("working") == t.stateTone("working"), "\(mode): working is a named token")
        }
        var row = SpacesRow(id: "agent:t", kind: .tab, tone: "working", title: "chat", face: SpacesRow.Face(initial: "C", tint: 0, avatar: nil))
        precondition(row.faceDot == "working", "a working face dot is orange")
        row.tone = "done"; precondition(row.faceDot == "ok", "a finished face dot is green")
        row.tone = "mute"; precondition(row.faceDot == nil, "idle face shows no dot")
        // Shapes as the TUI's state_icons draw them: working a circle, done and blocked a square.
        for (status, glyph) in [("working", "●"), ("done", "■"), ("blocked", "■")] {
            precondition(SpacesTree.statusGlyph(status) == glyph, "\(status) draws \(glyph), got \(SpacesTree.statusGlyph(status))")
        }
        print("PASS state_colors: dark+light working orange, done green, blocked bad, idle mute; face dots match; working ●, done and blocked ■")
    }
}
''')
    driver = build / "state_colors"
    subprocess.run(["swiftc", "-parse-as-library", str(SRC / "Theme.swift"), str(SRC / "GhosttyConfigMerge.swift"), str(SRC / "ShellTokens.swift"), str(SRC / "SpacesTree.swift"), str(source), "-o", str(driver)], check=True, timeout=240)
    subprocess.run([str(driver)], check=True, timeout=60)
