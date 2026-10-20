import Foundation

/// Click policy: a refusal never opens a cached target; activation is authoritative.
enum TerminalLinkDecision {
    static func needsResolution(cachedURL: String?) -> Bool { cachedURL == nil }

    static func openTarget(resolved: String?, activated: String?, handled: Bool, succeeded: Bool = true) -> String? {
        guard succeeded, !handled else { return nil }
        return activated ?? resolved
    }

    static func hoverTarget(serverURL: String?, fragment: String) -> String { serverURL ?? fragment }
}
