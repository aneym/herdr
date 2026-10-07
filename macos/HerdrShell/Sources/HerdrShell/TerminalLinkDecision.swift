import Foundation

/// Click policy kept independent of AppKit/socket timing. A resolved client target
/// remains authoritative if activation refuses or returns a different target; it gives
/// way only to an activation that continues it, since it holds just the visible cells.
enum TerminalLinkDecision {
    static func needsResolution(cachedURL: String?) -> Bool { cachedURL == nil }

    static func openTarget(resolved: String?, activated: String?, handled: Bool) -> String? {
        if handled { return nil }
        guard let resolved else { return activated }
        if let activated, activated.count > resolved.count, activated.hasPrefix(resolved) { return activated }
        return resolved
    }
}
