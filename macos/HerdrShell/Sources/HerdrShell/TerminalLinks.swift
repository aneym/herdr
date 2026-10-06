import AppKit

/// Cmd-hover and Cmd-click links in one terminal surface, resolved by the herdr server.
///
/// The surface shows the pane through `herdr terminal attach`, whose stream redraws each
/// row with cursor positioning and host autowrap off. libghostty therefore sees a
/// soft-wrapped URL as unrelated rows: its link regex stops at the row edge, so the
/// first row hovers a truncated URL and the continuation rows nothing. The server keeps
/// the pane's real, wrap-aware screen and answers `pane.link.resolve` (regions) and
/// `pane.link.activate` (target) for a viewport cell, as the TUI's Ctrl+click does.
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
        set("")
    }

    /// True when a Cmd-click at `cell` lands on the resolved link: the click is the
    /// link's, it opens through the server's activation, and the pane never sees it.
    func activate(at cell: Cell) -> Bool {
        guard !url.isEmpty, regions.contains(where: { $0.contains(col: cell.col, row: cell.row) }) else { return false }
        let shift = NSEvent.modifierFlags.contains(.shift)
        let (commands, paneId) = (self.commands, self.paneId)
        DispatchQueue.global(qos: .userInitiated).async {
            guard let answer = commands.paneLinkActivate(paneId: paneId, row: cell.row, col: cell.col) else {
                log("link activate failed \(paneId) \(cell.row):\(cell.col)")
                return
            }
            guard !answer.handled, let target = answer.url, Self.webURL(target) != nil else { return }
            DispatchQueue.main.async { GhosttyRuntime.openLink(target, paneId: paneId, shift: shift) }
        }
        return true
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
                    self.apply(found ?? [], at: cell)
                } else {
                    self.request()
                }
            }
        }
    }

    private func apply(_ found: [HerdrCommands.LinkRegion], at cell: Cell) {
        let spans = found.sorted { ($0.row, $0.start_col) < ($1.row, $1.start_col) }
        guard spans.contains(where: { $0.contains(col: cell.col, row: cell.row) }), let readSpan else {
            regions = []
            set("")
            return
        }
        let text = spans.map(readSpan).joined()
        guard Self.webURL(text) != nil else {
            regions = []
            set("")
            return
        }
        regions = spans
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
