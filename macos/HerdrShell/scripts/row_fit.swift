import AppKit
import SwiftUI

// row_fit <fixture.json> [--png out.png] [--mode light|dark]
// Draws every Spaces row with the shell's own SpacesRowView at the sidebar's column
// width and prints "<width> <row id>". Exits 1 when a row lays out wider than the
// column, because one such row widens the whole sidebar and shifts every row.

struct Fixture: Decodable { let input: SpacesInput; let overlay: Overlay; var chrome: SpacesChrome; let now: Double }

func log(_ s: String) { FileHandle.standardError.write(Data((s + "\n").utf8)) }

let sidebarWidth: CGFloat = 300
/// Sidebar.rows pads 8 each side; the footer does the same.
let column = sidebarWidth - 16

@MainActor func run() throws -> Int32 {
    var args = Array(CommandLine.arguments.dropFirst())
    func option(_ name: String) -> String? {
        guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
        defer { args.removeSubrange(i...(i + 1)) }
        return args[i + 1]
    }
    let png = option("--png")
    let mode: Mode = option("--mode") == "light" ? .light : .dark
    let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: args[0])))
    let rows = SpacesTree.build(fixture.input, overlay: fixture.overlay, chrome: fixture.chrome, now: fixture.now)
    let fallback = TerminalTheme(dark: "", light: "", darkColors: TerminalTheme.fallbackDark, lightColors: TerminalTheme.fallbackLight)
    let t = ThemeStore.tokens(for: mode, terminal: fallback)
    let firstSpace = rows.first { $0.kind == .space }?.id

    var wide = 0
    for row in rows {
        let view = SpacesRowView(row: row, t: t, firstSpaceId: firstSpace, selected: row.tab == fixture.input.focusedTab,
                                 focusMark: row.kind == .section && row.toggleKey != nil ? "◎" : nil,
                                 goal: row.kind == .goal ? AnyView(Text("All")) : nil)
            .font(.system(size: 12.5))
        let r = ImageRenderer(content: view)
        r.proposedSize = ProposedViewSize(width: column, height: nil)
        let w = (r.cgImage.map { CGFloat($0.width) / r.scale }) ?? 0
        if w > column + 0.5 { wide += 1 }
        print(String(format: "%6.1f %@", w, row.id))
    }

    if let png {
        let body = rows.filter { $0.kind != .footerUsage && $0.kind != .footerHost }
        let footer = rows.filter { $0.kind == .footerUsage || $0.kind == .footerHost }
        let sheet = VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 1) {
                ForEach(body) { row in
                    SpacesRowView(row: row, t: t, firstSpaceId: firstSpace, selected: row.tab == fixture.input.focusedTab,
                                  focusMark: row.kind == .section && row.toggleKey != nil ? "◎" : nil,
                                  goal: row.kind == .goal ? AnyView(Text("All")) : nil)
                }
            }.padding(.horizontal, 8).padding(.top, 10)
            Spacer(minLength: 12)
            // As Sidebar.footerGroup when the one-line form does not fit.
            VStack(alignment: .leading, spacing: 0) {
                ForEach(footer) { SpacesRowView(row: $0, t: t) }
            }.padding(.horizontal, 8).padding(.bottom, 8)
        }
        .font(.system(size: 12.5)).foregroundStyle(t.ink)
        // The sidebar is a fixed 300 pt NSView: wider content is centered in it and clipped.
        .frame(width: sidebarWidth).clipped()
        .background(t.panel)
        .environment(\.colorScheme, mode == .dark ? .dark : .light)
        let r = ImageRenderer(content: sheet)
        r.scale = 2
        guard let image = r.cgImage,
              let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else { return 2 }
        try data.write(to: URL(fileURLWithPath: png))
    }
    if wide > 0 { log("FAIL: \(wide) row(s) wider than the \(Int(column)) pt column") }
    return wide > 0 ? 1 : 0
}

@main struct RowFit {
    static func main() {
        _ = NSApplication.shared
        let code = MainActor.assumeIsolated { (try? run()) ?? 2 }
        exit(code)
    }
}
