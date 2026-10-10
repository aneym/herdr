import Foundation

/// The Ghostty config text every surface gets: the shell's defaults, then Alex's own
/// config minus the stripped keys, then the enforced lines. Kept free of AppKit so a
/// check can compile it beside libghostty and read back what the surfaces run with.
enum GhosttyConfigMerge {
    /// Directive lines from Alex's own config that must never reach a surface: keybinds
    /// (the shell owns chords through its menu and keymap), includes, anything that
    /// would make a terminal pane translucent or change the shell's grid padding, and
    /// vsync (see `enforced`).
    static let strippedKeys: Set<String> = [
        "keybind", "config-file", "background-opacity", "background-blur", "background-blur-radius",
        "background-opacity-cells",
        "window-padding-x", "window-padding-y", "window-padding-balance",
        "window-vsync",
    ]

    /// Appended after everything else, so nothing can override them.
    ///
    /// `window-vsync = false` keeps libghostty from ever creating a CVDisplayLink. With
    /// vsync on, each renderer thread starts and stops a CVDisplayLink, and when the
    /// display sleeps or reconfigures overnight `CVDisplayLink::stop()` can block that
    /// thread for hours. The renderer then stops draining its mailbox, and the next
    /// backing-scale change on display wake (`ghostty_surface_set_content_scale` from
    /// `viewDidChangeBackingProperties`) blocks the main thread behind it: the app is
    /// Not Responding when Alex comes back (stackshot 2026-10-07 08:48, unified log
    /// 2026-10-10 04:31 ET). Without a display link the renderer draws from its own
    /// wakeups, as it does on Linux.
    static let enforced = [
        "background-opacity = 1",
        "window-padding-x = 8",
        "window-padding-y = 6",
        "window-padding-balance = false",
        "window-vsync = false",
    ]

    static func key(of line: String) -> String? {
        let t = line.trimmingCharacters(in: .whitespaces)
        if t.isEmpty || t.hasPrefix("#") { return nil }
        guard let eq = t.firstIndex(of: "=") else { return t }
        return t[..<eq].trimmingCharacters(in: .whitespaces)
    }

    static func merged(base: String, user: String?) -> String {
        var out = base.components(separatedBy: "\n")
        if let user {
            for line in user.components(separatedBy: "\n") {
                if let k = key(of: line), strippedKeys.contains(k) { continue }
                out.append(line)
            }
        }
        out.append(contentsOf: enforced)
        return out.joined(separator: "\n") + "\n"
    }
}
