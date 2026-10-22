import AppKit

/// Bounded URL-opening trace for the dev test hook, independent of attention state.
final class OpenedLinks {
    static let shared = OpenedLinks()
    private(set) var urls: [String] = []

    func record(_ url: String) {
        urls.append(url)
        if urls.count > 10 { urls.removeFirst(urls.count - 10) }
    }
}

enum ReviewDocOpener {
    static let asideBundle = "at.studio.AsideBrowser"

    static func url(_ doc: TabDoc) -> URL? {
        if doc.url.hasPrefix("/") { return URL(fileURLWithPath: doc.url) }
        guard let url = URL(string: doc.url), ["http", "https"].contains(url.scheme?.lowercased() ?? "") else { return nil }
        return url
    }

    static func open(_ doc: TabDoc) {
        guard let url = url(doc) else { log("review doc: bad url \(doc.url)"); return }
        open(url)
    }

    static func open(_ url: URL) {
        OpenedLinks.shared.record("aside " + url.absoluteString)
        if agentRun {
            log("open ignored (--agent-run) aside \(url.absoluteString)")
            return
        }
        guard let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: asideBundle) else {
            log("aside: not installed, default browser")
            NSWorkspace.shared.open(url)
            return
        }
        NSWorkspace.shared.open([url], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration()) { _, error in
            if let error { log("aside: open failed \(error.localizedDescription)") }
        }
    }
}
