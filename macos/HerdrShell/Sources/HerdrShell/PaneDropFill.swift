import SwiftUI

struct PaneDropFill: NSViewRepresentable {
    let row: String
    let tokens: Tokens
    func makeNSView(context: Context) -> NSView { NSView() }
    func updateNSView(_ view: NSView, context: Context) {
        view.identifier = NSUserInterfaceItemIdentifier("pane-drop:" + row)
        view.wantsLayer = true
        view.layer?.cornerRadius = ShellRadius.row
        view.layer?.backgroundColor = tokens.accentNS.withAlphaComponent(tokens.mode == .dark ? ShellMotion.zoneFillAlphaDark : ShellMotion.zoneFillAlphaLight).cgColor
    }
    static func drawn(in view: NSView?, row: String) -> [String: Any] {
        guard let view else { return [:] }
        if view.identifier?.rawValue == "pane-drop:" + row, let layer = view.layer {
            return ["row": row, "alpha": layer.backgroundColor?.alpha ?? 0, "width": view.bounds.width, "height": view.bounds.height]
        }
        for child in view.subviews {
            let result = drawn(in: child, row: row)
            if !result.isEmpty { return result }
        }
        return [:]
    }
}
