import AppKit

/// A draggable split divider, derived from herdr's own layout snapshot
/// (`layouts[].splits` plus the pane rects on either side). The app never keeps a
/// layout of its own: a drag is turned into `pane.resize` calls and the panes are
/// rebuilt from the layout herdr answers with.
struct PaneDivider {
    let splitId: String
    /// True for a vertical line (children side by side, split direction "right"),
    /// false for a horizontal line (children stacked, split direction "down").
    let vertical: Bool
    let ratio: Double
    let splitRect: Snapshot.Rect
    /// A pane on the first-child side and one on the second-child side that touch the line.
    let firstPane: String
    let secondPane: String
    /// Line position in herdr layout units, along the axis it moves on.
    let pos: Double

    /// Size of the split along the axis the line moves on, in layout units.
    var extent: Double { vertical ? splitRect.width : splitRect.height }

    /// herdr direction and pane that move this line toward `delta` > 0 (right/down) or < 0.
    /// Growing the first child is "right"/"down" on a first-side pane; shrinking it is
    /// "left"/"up" on a second-side pane (herdr resizes the split on that pane's edge).
    func resizeCall(for delta: Double) -> (pane: String, direction: String) {
        delta > 0 ? (firstPane, vertical ? "right" : "down") : (secondPane, vertical ? "left" : "up")
    }

    static func from(_ layout: Snapshot.Layout) -> [PaneDivider] {
        let tol = 1.5
        func inside(_ r: Snapshot.Rect, _ outer: Snapshot.Rect) -> Bool {
            r.x >= outer.x - 0.5 && r.y >= outer.y - 0.5
                && r.x + r.width <= outer.x + outer.width + 0.5 && r.y + r.height <= outer.y + outer.height + 0.5
        }
        func overlap(_ a0: Double, _ al: Double, _ b0: Double, _ bl: Double) -> Bool {
            min(a0 + al, b0 + bl) - max(a0, b0) > 0
        }
        var out: [PaneDivider] = []
        for sp in layout.splits ?? [] {
            let vertical = sp.direction == "right"
            let pos = vertical ? sp.rect.x + sp.rect.width * sp.ratio : sp.rect.y + sp.rect.height * sp.ratio
            let panes = layout.panes.filter { inside($0.rect, sp.rect) }
            var found: (LayoutPaneRef, LayoutPaneRef, Double)?
            for a in panes {
                for b in panes where a.pane_id != b.pane_id {
                    let adjacent = vertical
                        ? abs(a.rect.x + a.rect.width - b.rect.x) <= tol && overlap(a.rect.y, a.rect.height, b.rect.y, b.rect.height)
                        : abs(a.rect.y + a.rect.height - b.rect.y) <= tol && overlap(a.rect.x, a.rect.width, b.rect.x, b.rect.width)
                    let line = vertical ? b.rect.x : b.rect.y
                    if adjacent, abs(line - pos) <= tol { found = (LayoutPaneRef(a.pane_id), LayoutPaneRef(b.pane_id), line); break }
                }
                if found != nil { break }
            }
            guard let (a, b, line) = found else { continue }
            out.append(PaneDivider(splitId: sp.id, vertical: vertical, ratio: sp.ratio, splitRect: sp.rect,
                                   firstPane: a.id, secondPane: b.id, pos: line))
        }
        return out
    }

    private struct LayoutPaneRef { let id: String; init(_ id: String) { self.id = id } }
}

/// The grab strip over one divider. It sits above the surfaces (the visible gap between
/// panes is 1 pt; the strip is wider) and reports drags in host coordinates.
final class DividerHandleView: NSView {
    enum Phase { case began, moved, ended }
    var divider: PaneDivider
    var onDrag: ((PaneDivider, Phase, CGFloat) -> Void)?
    private var startPoint: NSPoint = .zero
    private(set) var dragging = false

    init(divider: PaneDivider) {
        self.divider = divider
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override var acceptsFirstResponder: Bool { false }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: divider.vertical ? .resizeLeftRight : .resizeUpDown)
    }

    private func hostPoint(_ e: NSEvent) -> NSPoint { (superview ?? self).convert(e.locationInWindow, from: nil) }

    private func along(_ p: NSPoint) -> CGFloat { divider.vertical ? p.x - startPoint.x : p.y - startPoint.y }

    override func mouseDown(with event: NSEvent) {
        startPoint = hostPoint(event)
        dragging = true
        onDrag?(divider, .began, 0)
    }

    override func mouseDragged(with event: NSEvent) {
        guard dragging else { return }
        onDrag?(divider, .moved, along(hostPoint(event)))
    }

    override func mouseUp(with event: NSEvent) {
        guard dragging else { return }
        dragging = false
        onDrag?(divider, .ended, along(hostPoint(event)))
    }
}

/// Turns a divider drag into `pane.resize` requests, one in flight at a time. The target
/// ratio is absolute (ratio at drag start plus pixels moved over the split's pixel size), and
/// each request sends only what is still missing from the ratio herdr last reported, so
/// rounding, clamping and slow replies never accumulate error.
final class PaneResizeController {
    private let client: HerdrCommands
    private let queue = DispatchQueue(label: "herdr.pane-resize")
    /// Latest ratio herdr reported for a split id (main thread).
    var currentRatio: (String) -> Double? = { _ in nil }
    /// A layout herdr answered with; the window rebuilds its panes from it (main thread).
    var onLayout: (Snapshot.Layout) -> Void = { _ in }
    /// Called once when the drag has ended and the last request has come back.
    var onIdle: () -> Void = {}

    private var dragging = false
    private var inFlight = false
    private var startRatio = 0.5
    private var want: (divider: PaneDivider, ratio: Double)?
    private(set) var requestsSent = 0
    private(set) var lastError: String?

    /// True while a drag is in progress or a resize is still in flight; snapshots must not
    /// rebuild the panes meanwhile (a stale one would snap the divider back).
    var isBusy: Bool { dragging || inFlight }

    init(client: HerdrCommands) { self.client = client }

    func handle(_ divider: PaneDivider, _ phase: DividerHandleView.Phase, deltaPx: CGFloat, extentPx: CGFloat) {
        switch phase {
        case .began:
            dragging = true
            startRatio = currentRatio(divider.splitId) ?? divider.ratio
            want = (divider, startRatio)
        case .moved:
            guard extentPx > 0 else { return }
            want = (divider, min(0.9, max(0.1, startRatio + Double(deltaPx / extentPx))))
            pump()
        case .ended:
            if extentPx > 0 { want = (divider, min(0.9, max(0.1, startRatio + Double(deltaPx / extentPx)))) }
            dragging = false
            pump()
            if !inFlight { finish() }
        }
    }

    private func finish() {
        want = nil
        onIdle()
    }

    private func pump() {
        guard !inFlight, let (d, target) = want else { return }
        let cur = currentRatio(d.splitId) ?? d.ratio
        let delta = target - cur
        guard abs(delta) >= (dragging ? 0.004 : 0.0005) else { return }
        let call = d.resizeCall(for: delta)
        inFlight = true
        requestsSent += 1
        let amount = abs(delta)
        queue.async { [client] in
            let r = client.paneResize(paneId: call.pane, direction: call.direction, amount: amount)
            DispatchQueue.main.async { [self] in
                inFlight = false
                if let r {
                    lastError = nil
                    onLayout(r.layout)
                    // A resize herdr clamped or refused would repeat forever: stop chasing it.
                    if !r.changed { want = nil }
                } else {
                    lastError = "pane.resize failed"
                    want = nil
                }
                pump()
                if !inFlight && !dragging { finish() }
            }
        }
    }
}
