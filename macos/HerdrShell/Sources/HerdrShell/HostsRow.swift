import SwiftUI

/// One line of the hosts row: herdr's tab count for a host, plus whatever the provider adds.
struct HostRowData: Equatable, Identifiable {
    let host: String
    let tabs: Int
    let stats: HostStats?
    var id: String { host }

    /// Text shown after the host name, e.g. `2 tabs · slots 3/8 · 5 sessions`.
    /// The state dump carries the same string, so checks assert what the user sees.
    var detail: String {
        var parts = ["\(tabs) tab\(tabs == 1 ? "" : "s")"]
        if let s = stats {
            switch (s.slotsUsed, s.slotsTotal) {
            case let (u?, t?): parts.append("slots \(u)/\(t)")
            case let (u?, nil): parts.append("slots \(u)")
            case let (nil, t?): parts.append("slots -/\(t)")
            default: break
            }
            if let n = s.sessions { parts.append("\(n) session\(n == 1 ? "" : "s")") }
        }
        return parts.joined(separator: " · ")
    }
}

/// Per-host counts for the sidebar. Tab counts come from herdr (`host` tokens, via
/// `HerdrModel.hosts`); slots and sessions come from the `HostsProvider`. A host the
/// provider knows about but no tab uses still gets a row with 0 tabs.
final class HostsModel: ObservableObject {
    @Published private(set) var rows: [HostRowData] = []
    let provider: HostsProvider
    private var tabCounts: [(String, Int)] = []

    init(provider: HostsProvider) {
        self.provider = provider
        provider.onChange = { [weak self] in self?.rebuild() }
    }

    func start() { provider.start() }

    /// Called by HerdrModel on every applied snapshot.
    func update(tabCounts: [(String, Int)]) {
        self.tabCounts = tabCounts
        rebuild()
    }

    private func rebuild() {
        let stats = provider.stats
        var out = tabCounts.map { HostRowData(host: $0.0, tabs: $0.1, stats: stats[$0.0]) }
        let seen = Set(tabCounts.map { $0.0 })
        for host in stats.keys.sorted() where !seen.contains(host) {
            out.append(HostRowData(host: host, tabs: 0, stats: stats[host]))
        }
        if out != rows { rows = out }
    }
}

/// Bottom block of the sidebar: one line per host, then the poll or offline status.
struct HostsRow: View {
    @ObservedObject var hosts: HostsModel
    let tokens: Tokens
    let status: String
    let statusIsError: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack {
                Text("HOSTS").font(.system(size: 10.5, weight: .semibold)).tracking(0.8).foregroundStyle(tokens.mute)
                Spacer()
                Text(status).foregroundStyle(statusIsError ? tokens.warn : tokens.mute)
            }
            ForEach(hosts.rows) { h in
                HStack(spacing: 6) {
                    Text(h.host).foregroundStyle(h.host == TabClassifier.defaultHost ? tokens.ink : tokens.warn)
                    Spacer(minLength: 4)
                    Text(h.detail).foregroundStyle(tokens.mute).lineLimit(1)
                }
            }
            if Channel.kind == .dev {
                Text("DEV")
                    .font(.system(size: 9, weight: .semibold))
                    .tracking(0.6)
                    .foregroundStyle(tokens.warn)
            }
        }
        .font(.system(size: 11, design: .monospaced))
        .padding(.horizontal, 10).padding(.vertical, 7)
    }
}
