import Foundation

/// Click policy kept independent of AppKit/socket timing. A resolved client target
/// remains authoritative if activation refuses or returns a different target.
enum TerminalLinkDecision {
    static func needsResolution(cachedURL: String?) -> Bool { cachedURL == nil }

    static func openTarget(resolved: String?, activated: String?, handled: Bool) -> String? {
        if handled { return nil }
        if let resolved { return resolved }
        return activated
    }
}
