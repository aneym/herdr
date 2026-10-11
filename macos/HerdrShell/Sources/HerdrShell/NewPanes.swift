import Foundation

/// Panes another client opened while this app was running.
///
/// Alex, 2026-10-11: "on book anytime i start a new thread it loses focus and i need to click
/// back in to take back control, new threads should be focused on the client that made them
/// when they're first made." Every Shell showing the tab attaches a new pane as soon as its
/// snapshot lists it, and a refused attach takes over (the sleeping-machine rule), so the
/// last client to attach won, usually an idle machine.
///
/// A pane is another client's new pane when it appears in a snapshot after the first one from
/// its machine, this app did not create it, and it appeared less than `window` seconds ago.
/// For such a pane this app does not take over a refused attach, and focusing it here does not
/// take it back; a key or a click on it still does. Panes present when the app starts, or
/// after that window, keep the hand-off rules in SurfaceRegistry.
final class NewPanes {
    static let shared = NewPanes()
    static let window: TimeInterval = 60

    private let lock = NSLock()
    private var baselined: Set<String> = []
    private var seen: Set<String> = []
    private var appeared: [String: Date] = [:]
    private var mine: [String: Date] = [:]

    /// Every pane id one machine's snapshot lists (`source` "" for this machine's server).
    func observe(source: String, paneIds: [String], now: Date = Date()) {
        lock.lock(); defer { lock.unlock() }
        let first = baselined.insert(source).inserted
        for id in paneIds where seen.insert(id).inserted && !first {
            appeared[id] = now
        }
        appeared = appeared.filter { now.timeIntervalSince($0.value) < Self.window }
    }

    /// A pane this app's own tab.create or pane.split made.
    func markMine(_ paneId: String) {
        lock.lock(); defer { lock.unlock() }
        mine[paneId] = Date()
    }

    /// True for a pane this app made less than `window` seconds ago: its attach takes it over
    /// at once, so keys typed as it opens are never lost to a refused attach (an idle client
    /// can attach first).
    func isMine(_ paneId: String, now: Date = Date()) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return mine[paneId].map { now.timeIntervalSince($0) < Self.window } ?? false
    }

    /// True while the pane is another client's new pane: leave control with its creator.
    func othersNew(_ paneId: String, now: Date = Date()) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard mine[paneId] == nil, let at = appeared[paneId] else { return false }
        return now.timeIntervalSince(at) < Self.window
    }

    /// Forget everything (tests).
    func reset() {
        lock.lock(); defer { lock.unlock() }
        baselined = []; seen = []; appeared = [:]; mine = [:]
    }
}
