import Foundation

enum LinkRoute {
    case aside, external, ignore

    static func decide(_ url: URL, shift: Bool) -> LinkRoute {
        switch url.scheme?.lowercased() {
        case "http", "https", "file": return shift ? .external : .aside
        case "mailto": return .external
        default: return .ignore
        }
    }
}
