import Foundation

/// Shared menu availability and response handling, independent of the presentation surface.
enum PaneRestart {
    enum Next: Equatable { case done, confirm, error(String) }
    static func enabled(hasAgent: Bool) -> Bool { hasAgent }
    static func message(code: String, fallback: String, reason: String? = nil) -> String {
        switch code {
        case "not_resumable", "no_session": return "This agent can't be resumed: no saved chat found."
        case "unsupported": return "Restart isn't supported for this agent yet."
        case "start_failed": return "The agent didn't come back up. Check the pane for errors."
        case "busy": return (reason.map { $0 == "restart_pending" } ?? fallback.contains("previous restart")) ? "This agent is already restarting. Wait for it to finish." : "This agent is blocked. Resolve its prompt before restarting."
        default: return fallback
        }
    }
    static func next(code: String?, message: String, forced: Bool, reason: String? = nil) -> Next {
        guard let code else { return .done }
        return code == "busy" && (reason.map { $0 == "working" } ?? message.hasSuffix(" is Working")) && !forced ? .confirm : .error(message)
    }
}
