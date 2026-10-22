import Combine
import Foundation

final class ThoughtsModel: ObservableObject {
    @Published private(set) var count = 0
    private var timer: Timer?

    func start() {
        guard timer == nil else { return }
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 20, repeats: true) { [weak self] _ in self?.refresh() }
    }

    func refresh() {
        RemoteActions.run(verb: "thoughts", args: []) { [weak self] ok, _ in
            if ok, let count = RemoteActions.last["count"] as? Int { self?.count = count }
        }
    }

    func save(_ text: String, done: @escaping (Bool, String) -> Void) {
        let label = ProcessInfo.processInfo.hostName.components(separatedBy: ".")[0].lowercased()
        let filtered = label.filter { $0.isASCII && ($0.isLowercase || $0.isNumber || $0 == "-") }
        let source = String((filtered.isEmpty ? "mac" : filtered).prefix(32))
        RemoteActions.run(verb: "thought", args: ["--source=\(source)"], stdin: text) { [weak self] ok, message in
            if ok {
                if let count = RemoteActions.last["count"] as? Int { self?.count = count }
                self?.refresh()
            }
            done(ok, message)
        }
    }
}
