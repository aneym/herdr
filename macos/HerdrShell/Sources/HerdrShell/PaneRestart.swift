import Foundation

/// Shared menu availability and response handling, independent of the presentation surface.
enum PaneRestart {
    enum Next: Equatable { case done, confirm, error(String) }
    struct Tracking {
        private var deadlines: [String: Date] = [:]
        // Server startup polling lasts 5s; retain a 25s shutdown/delivery margin.
        mutating func request(_ key: String, now: Date = Date()) { deadlines[key] = now.addingTimeInterval(30) }
        mutating func clear(_ key: String) { deadlines.removeValue(forKey: key) }
        mutating func expire(now: Date = Date()) { deadlines = deadlines.filter { $0.value > now } }
        mutating func observe(_ key: String, restoreError: String?, changed: Bool, now: Date = Date()) -> Bool {
            expire(now: now)
            guard deadlines[key] != nil else { return false }
            if restoreError != nil && changed { clear(key); return true }
            return false
        }
    }
    static func requestKey(server: String, pane: String) -> String { server + ":" + pane }
    static func enabled(hasAgent: Bool) -> Bool { hasAgent }
    static func message(code: String, fallback: String, reason: String? = nil) -> String {
        switch code {
        case "not_resumable", "no_session": return "This agent can't be resumed: no saved chat found."
        case "unsupported": return "Restart isn't supported for this agent yet."
        case "start_failed": return "The agent didn't come back up. Check the pane for errors."
        case "busy":
            if reason == "restart_pending" || reason == nil && fallback.contains("previous restart") { return "This agent is already restarting. Wait for it to finish." }
            if reason == "blocked" || reason == nil && fallback.hasSuffix(" is Blocked") { return "This agent is blocked. Resolve its prompt before restarting." }
            return "This agent can't restart right now"
        default: return fallback
        }
    }
    static func next(code: String?, message: String, forced: Bool, reason: String? = nil) -> Next {
        guard let code else { return .done }
        return code == "busy" && (reason.map { $0 == "working" } ?? message.hasSuffix(" is Working")) && !forced ? .confirm : .error(message)
    }
}
