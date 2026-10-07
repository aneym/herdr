import Foundation

/// Shared menu availability and response handling, independent of the presentation surface.
enum PaneRestart {
    enum Next: Equatable { case done, confirm, error(String) }
    static func enabled(hasAgent: Bool) -> Bool { hasAgent }
    static func next(code: String?, message: String, forced: Bool) -> Next {
        guard let code else { return .done }
        return code == "busy" && !forced ? .confirm : .error(message)
    }
}
