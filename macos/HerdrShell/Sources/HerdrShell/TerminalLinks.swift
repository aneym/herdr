import AppKit

/// Cmd-hover and Cmd-click links in one terminal surface, resolved by the herdr server.
///
/// The surface shows the pane through `herdr terminal attach`, whose stream redraws each
/// row with cursor positioning and host autowrap off. libghostty therefore sees a
/// soft-wrapped URL as unrelated rows: its link regex stops at the row edge, so the
/// first row hovers a truncated URL and the continuation rows nothing. The server keeps
/// the pane's real, wrap-aware screen and answers `pane.link.resolve` (guards/target) and
/// `pane.link.activate` (guarded target) for a viewport cell, as the TUI's Ctrl+click does.
///
/// One request at a time, and only when the cell under the pointer changes; a cell that
/// changed while a request was out is asked next. Any miss (no link, a pane the server
/// does not show, an older server) leaves libghostty's own link handling in place.
final class TerminalLinks {
    typealias Cell = (col: Int, row: Int)

    private let paneId: String
    private let commands: HerdrCommands
    /// Text of one viewport row span on this surface (main thread). The attach grid and
    /// the server's viewport are the same cells, so the server's regions read the URL.
    var readSpan: ((HerdrCommands.LinkRegion) -> String)?
    /// The http(s) URL under the pointer, or "" when there is none (main thread).
    var onChange: ((String) -> Void)?

    private(set) var url = ""
    private var regions: [HerdrCommands.LinkRegion] = []
    private var resolution: HerdrCommands.LinkResolution?
    private var target: Cell?
    private var inFlight = false
    private var epoch = 0

    init(paneId: String, socketPath: String) {
        self.paneId = paneId
        self.commands = HerdrCommands(socketPath: socketPath)
    }

    /// Pointer at `cell` with Cmd held, or nil (Cmd up, pointer outside the grid).
    func hover(_ cell: Cell?) {
        guard let cell else { clear(); return }
        if let t = target, t == cell { return }
        target = cell
        request()
    }

    func clear() {
        epoch += 1
        target = nil
        regions = []
        resolution = nil
        set("")
    }

    /// Bind clicks to a cached resolve, or resolve first when hover has not completed.
    /// A refused activation consumes the gesture so native replay cannot open stale text.
    func activate(at cell: Cell, shift: Bool, completion: @escaping (Bool) -> Void) {
        let cached = regions.contains(where: { $0.contains(col: cell.col, row: cell.row) }) ? resolution : nil
        let cachedURL = cached == nil ? nil : url
        let (commands, paneId) = (self.commands, self.paneId)
        DispatchQueue.global(qos: .userInitiated).async {
            guard let found = cached ?? commands.paneLinkResolve(paneId: paneId, row: cell.row, col: cell.col),
                  found.regions.contains(where: { $0.contains(col: cell.col, row: cell.row) }) else {
                DispatchQueue.main.async { completion(false) }
                return
            }
            let answer = commands.paneLinkActivate(paneId: paneId, row: cell.row, col: cell.col, resolution: found)
            let target = TerminalLinkDecision.openTarget(resolved: found.url ?? cachedURL, activated: answer?.url,
                                                         handled: answer?.handled ?? false, succeeded: answer != nil)
            DispatchQueue.main.async { [weak self] in
                guard let answer else {
                    self?.clear()
                    completion(true)
                    return
                }
                if let target, Self.webURL(target) != nil {
                    GhosttyRuntime.openLink(target, paneId: paneId, shift: shift)
                    completion(true)
                } else {
                    completion(answer.handled)
                }
            }
        }
    }

    private func request() {
        guard !inFlight, let cell = target else { return }
        inFlight = true
        let epoch = self.epoch
        let (commands, paneId) = (self.commands, self.paneId)
        DispatchQueue.global(qos: .userInitiated).async {
            let found = commands.paneLinkResolve(paneId: paneId, row: cell.row, col: cell.col)
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.inFlight = false
                guard let current = self.target else { return }
                if epoch == self.epoch, current == cell {
                    self.apply(found, at: cell)
                } else {
                    self.request()
                }
            }
        }
    }

    private func apply(_ found: HerdrCommands.LinkResolution?, at cell: Cell) {
        let spans = (found?.regions ?? []).sorted { ($0.row, $0.start_col) < ($1.row, $1.start_col) }
        guard let found, spans.contains(where: { $0.contains(col: cell.col, row: cell.row) }) else {
            regions = []
            resolution = nil
            set("")
            return
        }
        // Only older servers need visible-cell reconstruction. A clipped prefix is
        // not a URL, while the server target follows wraps into scrollback.
        let fragment = found.url == nil ? spans.map { readSpan?($0) ?? "" }.joined() : ""
        let text = TerminalLinkDecision.hoverTarget(serverURL: found.url, fragment: fragment)
        guard Self.webURL(text) != nil else {
            regions = []
            resolution = nil
            set("")
            return
        }
        regions = spans
        resolution = found
        set(text)
    }

    private func set(_ value: String) {
        guard url != value else { return }
        url = value
        onChange?(value)
    }

    /// Plain-text links open only as http or https.
    static func webURL(_ text: String) -> URL? {
        guard let url = URL(string: text), let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https", url.host != nil else { return nil }
        return url
    }
}
