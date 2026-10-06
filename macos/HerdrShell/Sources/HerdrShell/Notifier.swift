import AppKit
import UserNotifications

/// One macOS notification when a tab Alex is not looking at starts needing him,
/// plus the dock count of those tabs. Agent runs record the same facts and never post:
/// a check must not raise a banner on the machine it is driving.
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    static let shared = Notifier()

    struct Note {
        var tab: String
        var kind: String
        var title: String
    }

    /// Per-pane status the last snapshot already applied. A transition is a change from this.
    struct Facts {
        var seen = false
        var labels: [String: String] = [:]
        var parked: Set<String> = []
        /// tab -> pane -> (status, terminal title)
        var panes: [String: [String: (status: String, title: String)]] = [:]
        static let empty = Facts()
    }

    private(set) var openedURLs: [String] = []
    private(set) var notifications: [Note] = []
    private(set) var dockBadge = ""
    private(set) var latest = Facts.empty

    /// `--notify` / `HERDR_NOTIFY` / defaults key `notify`. Default on.
    var enabled = true
    var onSelect: ((String) -> Void)?

    private var lastSent: [String: Date] = [:]
    private var auth: Auth = .unknown
    private var pending: [(title: String, body: String, tab: String)] = []

    private enum Auth { case unknown, asking, granted, denied }

    /// Coalesce bursts, and don't nag the same tab more than once a minute.
    private let coalesce: TimeInterval = 3
    private let perTab: TimeInterval = 60

    func recordOpened(_ url: String) {
        openedURLs.append(url)
        if openedURLs.count > 10 { openedURLs.removeFirst(openedURLs.count - 10) }
    }

    func observe(old: Facts, new: Facts, selected: String?, windowKey: Bool) {
        latest = new
        updateBadge(new)
        guard enabled, old.seen else { return }
        for (tab, panes) in new.panes {
            if new.parked.contains(tab) { continue }
            if windowKey, tab == selected { continue }
            var kind: String?
            var term = ""
            for (pane, fact) in panes {
                guard let prev = old.panes[tab]?[pane]?.status else { continue }
                if fact.status == "blocked", prev != "blocked" {
                    kind = "blocked"
                    term = fact.title
                    break
                }
            }
            if kind == nil {
                for (pane, fact) in panes {
                    guard old.panes[tab]?[pane]?.status == "working", fact.status == "done" else { continue }
                    kind = "done"
                    term = fact.title
                    break
                }
            }
            guard let kind else { continue }
            let title = new.labels[tab] ?? tab
            let verb = kind == "blocked" ? "needs you" : "finished"
            let body = term.isEmpty ? verb : "\(verb) \(term)"
            emit(tab: tab, kind: kind, title: title, body: body, limit: true)
        }
    }

    /// OSC 9 / 777. Same post path as an agent transition; `tab` is the pane's tab when we know it.
    func desktop(title: String, body: String, tab: String) {
        guard enabled else { return }
        let shown = title.isEmpty ? body : title
        emit(tab: tab, kind: "osc", title: shown, body: body.isEmpty ? shown : body, limit: false)
    }

    func tabId(forPane pane: String, in facts: Facts) -> String? {
        for (tab, panes) in facts.panes where panes[pane] != nil { return tab }
        return nil
    }

    private func emit(tab: String, kind: String, title: String, body: String, limit: Bool) {
        if limit, let at = lastSent[tab] {
            let dt = Date().timeIntervalSince(at)
            if dt < coalesce || dt < perTab { return }
        }
        if limit { lastSent[tab] = Date() }
        notifications.append(Note(tab: tab, kind: kind, title: title))
        if notifications.count > 20 { notifications.removeFirst(notifications.count - 20) }
        log("notify \(tab) \(kind) \(title)")
        guard !agentRun else { return }
        post(title: title, body: body, tab: tab)
    }

    private func updateBadge(_ facts: Facts) {
        var n = 0
        for (tab, panes) in facts.panes where !facts.parked.contains(tab) {
            if panes.values.contains(where: { $0.status == "blocked" || $0.status == "done" }) { n += 1 }
        }
        dockBadge = n == 0 ? "" : String(n)
        NSApp.dockTile.badgeLabel = dockBadge.isEmpty ? nil : dockBadge
    }

    /// Banners only from a real bundle. A bare `swift build` binary has no bundle id; log and stop.
    private func post(title: String, body: String, tab: String) {
        guard Bundle.main.bundleIdentifier != nil else {
            log("notify unavailable (no bundle)")
            return
        }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        switch auth {
        case .granted:
            deliver(title: title, body: body, tab: tab)
        case .denied:
            return
        case .asking:
            pending.append((title, body, tab))
        case .unknown:
            auth = .asking
            pending.append((title, body, tab))
            center.requestAuthorization(options: [.alert, .sound]) { [weak self] granted, err in
                if let err { log("notify auth \(err.localizedDescription)") }
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.auth = granted ? .granted : .denied
                    let queued = self.pending
                    self.pending = []
                    guard granted else { return }
                    for item in queued { self.deliver(title: item.title, body: item.body, tab: item.tab) }
                }
            }
        }
    }

    private func deliver(title: String, body: String, tab: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.userInfo = ["tab": tab]
        let req = UNNotificationRequest(identifier: "herdr.\(tab).\(UUID().uuidString)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(req) { err in
            if let err { log("notify post \(err.localizedDescription)") }
        }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound])
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        let tab = response.notification.request.content.userInfo["tab"] as? String ?? ""
        DispatchQueue.main.async {
            NSApp.activate(ignoringOtherApps: true)
            if !tab.isEmpty { self.onSelect?(tab) }
        }
        completionHandler()
    }

    static func capture(snapshot: Snapshot?, parked: Set<String>) -> Facts {
        guard let snapshot else { return .empty }
        var facts = Facts(seen: true, parked: parked)
        for t in snapshot.tabs {
            facts.labels[t.tab_id] = t.label ?? "tab \(t.number)"
        }
        var agentStatus: [String: String] = [:]
        for a in snapshot.agents {
            if let s = a.agent_status { agentStatus[a.pane_id] = s }
        }
        for p in snapshot.panes {
            let status = p.agent_status ?? agentStatus[p.pane_id] ?? "unknown"
            let title = (p.terminal_title ?? p.title ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            facts.panes[p.tab_id, default: [:]][p.pane_id] = (status, title)
        }
        return facts
    }

    /// Flag, then `HERDR_NOTIFY`, then the defaults key `notify`. Anything but off stays on.
    static func enabled(flag: String?, env: String?) -> Bool {
        if let flag { return isOn(flag) }
        if let env { return isOn(env) }
        if Channel.store.object(forKey: "notify") == nil { return true }
        if let s = Channel.store.string(forKey: "notify") { return isOn(s) }
        return Channel.store.bool(forKey: "notify")
    }

    private static func isOn(_ raw: String) -> Bool {
        let v = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return v != "off" && v != "0" && v != "false"
    }
}
