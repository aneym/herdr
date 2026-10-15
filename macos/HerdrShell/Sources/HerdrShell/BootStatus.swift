import Foundation

enum BootStatus {
    struct Line: Equatable {
        let text: String
        let centered: Bool
    }

    static func line(loaded: Bool, error: String?, pendingRestores: Int, waitedSeconds: Int) -> Line? {
        if !loaded {
            if error == nil { return Line(text: "Connecting to Herdr…", centered: true) }
            let waited = waitedSeconds >= 5 ? " · \(waitedSeconds) s" : ""
            return Line(text: "Waiting for the Herdr server to start\(waited)", centered: true)
        }
        if error != nil { return Line(text: "Reconnecting to Herdr…", centered: false) }
        if pendingRestores > 0 {
            let noun = pendingRestores == 1 ? "agent" : "agents"
            return Line(text: "Restoring \(pendingRestores) \(noun) · pinned first, open a chat to start it now", centered: false)
        }
        return nil
    }
}
