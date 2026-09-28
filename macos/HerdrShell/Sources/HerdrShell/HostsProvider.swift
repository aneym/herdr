import Foundation

/// What a host reports beyond herdr's own tabs: fold-runner slots and box-registry
/// session counts. Every field is optional; a host with none shows tab counts only.
struct HostStats: Equatable {
    var slotsUsed: Int?
    var slotsTotal: Int?
    var sessions: Int?
}

/// Seam for the data herdr does not have: slots from the fold runner and session counts
/// from the box registry. The default, `HerdrOnlyHostsProvider`, reports nothing, so the
/// hosts row shows per-host tab counts from `host` tokens alone. A real provider
/// implements this protocol and is passed to `HerdrModel`; nothing else changes.
protocol HostsProvider: AnyObject {
    var name: String { get }
    /// Latest stats by host name (the `host` token value). Read on the main thread.
    var stats: [String: HostStats] { get }
    /// Set by the consumer; the provider calls it on the main thread when `stats` changes.
    var onChange: (() -> Void)? { get set }
    func start()
    func stop()
}

/// Default: herdr-only data.
final class HerdrOnlyHostsProvider: HostsProvider {
    let name = "herdr-only"
    let stats: [String: HostStats] = [:]
    var onChange: (() -> Void)?
    func start() {}
    func stop() {}
}

/// Stub provider read from a JSON file (`--hosts-stub FILE`), for checks and demos:
///
///     {"forge-1": {"slots_used": 3, "slots_total": 8, "sessions": 5}, "PC": {"slots_used": 1}}
///
/// The file is re-read when its modification time changes (polled every 0.25 s), so a
/// check can change the numbers while the app runs. A missing or unreadable file means
/// no stats.
final class FileHostsProvider: HostsProvider {
    let name = "file-stub"
    private(set) var stats: [String: HostStats] = [:]
    var onChange: (() -> Void)?

    private let path: String
    private var timer: Timer?
    private var lastStamp: Date??

    init(path: String) { self.path = path }

    func start() {
        reload()
        timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in self?.reload() }
    }

    func stop() { timer?.invalidate(); timer = nil }

    private func reload() {
        let stamp = (try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate]) as? Date
        if let last = lastStamp, last == stamp { return }
        lastStamp = .some(stamp)
        let parsed = Self.parse(FileManager.default.contents(atPath: path))
        guard parsed != stats else { return }
        stats = parsed
        onChange?()
    }

    static func parse(_ data: Data?) -> [String: HostStats] {
        guard let data, let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return [:] }
        var out: [String: HostStats] = [:]
        for (host, v) in obj {
            guard let d = v as? [String: Any] else { continue }
            out[host] = HostStats(slotsUsed: d["slots_used"] as? Int, slotsTotal: d["slots_total"] as? Int,
                                  sessions: d["sessions"] as? Int)
        }
        return out
    }
}
