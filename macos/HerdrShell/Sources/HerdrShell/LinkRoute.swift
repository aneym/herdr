import Foundation

/// Link policy is independent of AppKit and of the desk's presentation.
enum LinkRoute {
    case desk, external, ignore

    static func decide(_ url: URL, shift: Bool) -> LinkRoute {
        switch url.scheme?.lowercased() {
        case "http", "https", "file": return shift ? .external : .desk
        case "mailto": return .external
        default: return .ignore
        }
    }
}
