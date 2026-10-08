import AppKit
import QuartzCore

/// Per-window gesture owner; snapshots and pane.place remain layout authority.
final class PaneDrag: NSObject {
    enum Phase: String { case idle, pressed, lifted, dropping, settling, cancelling }
    enum Zone: Equatable {
        case centre(String), paneEdge(String, PaneDropSide), tabEdge(PaneDropSide), intoTab(String), newTabIn(String)
        var json: [String: Any] {
            switch self {
            case .centre(let p): return ["kind": "centre", "target": p]
            case .paneEdge(let p, let s): return ["kind": "pane_edge", "target": p, "side": s.rawValue]
            case .tabEdge(let s): return ["kind": "tab_edge", "side": s.rawValue]
            case .intoTab(let t): return ["kind": "into_tab", "tab": t]
            case .newTabIn(let w): return ["kind": "new_tab_in", "workspace": w]
            }
        }
    }
    private weak var owner: MainWindowController?
    private(set) var phase: Phase = .idle
    private var source: String?, originTab: String?, label = ""
    private let chipBorder = CAShapeLayer()
    private var sourceGlyph: ShellState = .asleep
    private var lastHostSize = CGSize.zero
    private var zone: Zone?, lastZone: Zone?
    /// Check hook only (check_pane_drag_end.py): hold a drop's call before it is sent, as a slow link would, until
    /// `sendHeldDrop()`. Snapshots keep arriving meanwhile.
    var holdDrops = false
    /// Inject a failed network edge without sending a mutation (check hook only).
    var failNextDrop = false
    var loseNextDrop = false
    private var heldDrop: (() -> Void)?
    /// Check hook only: hold a drop's reply once it is in, until `sendHeldReply()`, so the drop's own snapshot can land
    /// first. A held reply outlives the drop's end, so a check can deliver it late.
    var holdReplies = false
    private var heldReply: (generation: Int, answer: () -> Void)?
    /// Replies handled, and how many of them came for a drop that had already ended (and so changed nothing).
    private var repliesHandled = 0, repliesIgnored = 0
    /// While a sent drop waits for its reply the host keeps the boxes it had at the release: snapshots that land
    /// meanwhile stay in the model (the newest wins) and only their caps apply. `frozenEpoch` tells whether one did.
    private(set) var holdsLayout = false
    private var frozenEpoch = 0
    /// The host size at the release: a pending drop holds its boxes and terminal sizes at this size.
    private var dropSize = CGSize.zero
    /// How long a sent drop waits for its reply, the same as the Windows Shell.
    static let replyTimeout: TimeInterval = 4
    private var pointer = CGPoint.zero, start = CGPoint.zero
    private var keyboard = false, keyboardTarget: String?
    private var sent: [[String: Any]] = [] {
        // Every call also lands in `sendLog`, which a new drag does not clear, so a check can see what a
        // drag's end sent even when the next drag begins in the same turn.
        didSet {
            guard sent.count > oldValue.count else { return }
            for call in sent[oldValue.count...] { sendSeq += 1; sendLog.append(call.merging(["seq": sendSeq]) { a, _ in a }) }
            if sendLog.count > 64 { sendLog.removeFirst(sendLog.count - 64) }
        }
    }
    private var sendLog: [[String: Any]] = [], sendSeq = 0
    /// The chip glyph's color as last painted (check hook).
    private var chipPaintedTone: UInt32?
    private var supported: [String: Bool] = [:], probing = Set<String>()
    private var generation = 0, layoutGeneration = 0
    private var cache: [String: CGRect] = [:], rejected = Set<String>()
    private var inFlight: String?
    private var ghost: CGRect?, ghostTarget: CGRect?, ghostSource = "estimate"
    private var boxes: [String: CGRect] = [:]
    var drawnBoxes: [String: CGRect] { boxes }
    private var monitor: Any?
    private var link: CADisplayLink?
    private var frozenMs: CGFloat?
    private struct Dwell {
        var tab: String, point: CGPoint
        var started = ProcessInfo.processInfo.systemUptime
        var frozenStart: CGFloat?
    }
    private var dwell: Dwell?
    private var springSelection = false, sprung = false
    // Keep a quick cancel's origin focus behind the spring's destination focus on the wire.
    private let tabFocusQueue = DispatchQueue(label: "herdr.pane-drag.tab-focus")
    var reduceOverride: Bool?
    private var reduce: Bool { reduceOverride ?? NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }
    private struct Motion {
        var pane: String?, kind: String, from: CGRect, to: CGRect
        var started = ProcessInfo.processInfo.systemUptime
        var duration: CGFloat
    }
    private var motions: [Motion] = []
    private let overlay = PaneDragOverlay(frame: .zero)
    var suppressMouse: Bool { phase == .lifted || phase == .dropping }
    init(owner: MainWindowController) {
        self.owner = owner
        super.init()
        overlay.drag = self
        owner.host.addSubview(overlay)
        probe()
    }
    deinit { if let monitor { NSEvent.removeMonitor(monitor) }; link?.invalidate() }
    private var socket: String { (source ?? owner?.state.selectedTab).flatMap { Machines.config(for: $0)?.socket } ?? owner?.commands.socketPath ?? "" }
    var placeSupported: Bool { supported[socket] == true }
    func machinesChanged() {
        guard let owner else { return }
        let id = source ?? owner.state.selectedTab ?? ""
        if let config = Machines.config(for: id), let machine = owner.model.machines.first(where: { $0.name == config.name }) {
            if machine.problem != nil { supported[config.socket] = nil; cancel() }
            else { probe() }
        }
    }
    func connectionChanged(online: Bool) {
        if !online { supported = [:]; cancel() } else { probe() }
    }
    func probe() {
        let path = socket
        guard supported[path] == nil, !probing.contains(path) else { return }
        probing.insert(path)
        let commands = HerdrCommands(socketPath: path)
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let reply = commands.panePlace(params: ["pane_id": "__probe__", "target": ["type": "pane", "pane_id": "__probe__"], "side": "right", "dry_run": true])
            let code = (reply?["error"] as? [String: Any])?["code"] as? String
            DispatchQueue.main.async {
                guard let self else { return }
                self.probing.remove(path)
                if reply != nil { self.supported[path] = code == "pane_not_found" || reply?["result"] != nil }
                else if self.socket == path { self.cancel() }
            }
        }
    }
    private func installMonitor() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .leftMouseDragged, .leftMouseUp, .rightMouseDown, .rightMouseUp]) { [weak self] e in
            guard let self, let owner = self.owner, e.window === owner.window else { return e }
            // A press on the panes ends a drop still waiting on its reply, quietly, before the window hit-tests it:
            // the press then lands on the flushed layout, so a cap press drags from there and a terminal press focuses.
            if e.type == .leftMouseDown {
                if self.phase == .dropping, owner.host.bounds.contains(owner.host.convert(e.locationInWindow, from: nil)) { self.cancel() }
                return e
            }
            if e.type == .keyDown { return self.handleKey(e) ? nil : e }
            if e.type == .rightMouseDown && self.phase != .idle { self.cancel(); return nil }
            if e.type == .rightMouseUp && self.phase == .cancelling { return nil }
            if self.phase == .pressed || self.phase == .lifted {
                if e.type == .leftMouseDragged { self.drag(toWindowPoint: e.locationInWindow); return nil }
                if e.type == .leftMouseUp { self.release(); return nil }
            }
            return e
        }
    }
    func press(pane: String, at point: CGPoint) {
        guard let owner else { return }
        // A new drag ends a drop still waiting on its reply, quietly.
        if phase == .dropping { cancel() }
        source = pane
        probe()
        guard placeSupported, owner.host.rects.count > 1, owner.shownLayout?.zoomed != true else {
            owner.focusPane(pane); return
        }
        resetVisuals()
        generation += 1; source = pane; originTab = owner.state.selectedTab
        label = owner.host.caps[pane]?.name ?? ""
        sourceGlyph = owner.host.caps[pane]?.glyph ?? .asleep
        sent = []; chipPaintedTone = nil; lastZone = nil; zone = nil; cache = [:]; rejected = []; inFlight = nil
        pointer = owner.host.convert(point, from: nil); start = pointer
        phase = .pressed; keyboard = false; installMonitor()
    }
    private func lift() {
        phase = .lifted; NSCursor.closedHand.set()
        addMotion(pane: source, kind: "fade", from: .zero, to: .zero, duration: ShellMotion.fadeMs)
        draw()
    }
    func drag(toWindowPoint point: CGPoint) {
        guard let owner, phase == .pressed || phase == .lifted else { return }
        pointer = owner.host.convert(point, from: nil)
        if phase == .pressed {
            guard hypot(pointer.x - start.x, pointer.y - start.y) >= ShellMotion.dragThresholdPx else { return }
            lift()
        }
        updateZone(at: point); draw()
    }
    private func updateZone(at windowPoint: CGPoint) {
        guard let owner else { return }
        let view = owner.sidebarHostView, safe = view.safeAreaRect, p = view.convert(windowPoint, from: nil)
        let inset = view.isFlipped ? safe.minY : view.bounds.height - safe.maxY
        let rowPoint = CGPoint(x: p.x - safe.minX, y: (view.isFlipped ? p.y : view.bounds.height - p.y) - inset)
        if owner.state.sidebarVisible, !view.isHidden, view.bounds.contains(p),
           let row = owner.state.rowFrames.first(where: { $0.value.contains(rowPoint) })?.key {
            let id = String(row.dropFirst(row.hasPrefix("tab:") ? 4 : 6))
            if (row.hasPrefix("tab:") || row.hasPrefix("space:")), PinDrag.machine(of: id) == PinDrag.machine(of: source ?? "") {
                updateDwell(tab: row.hasPrefix("tab:") ? id : nil)
                // The source tab remains a dwell target (spring back), never a drop target.
                setZone(row.hasPrefix("tab:") ? (id == originTab ? nil : .intoTab(id)) : .newTabIn(id)); return
            }
        }
        updateDwell(tab: nil)
        let ids = owner.host.rects.map { $0.0.paneId }, rects = owner.host.rects.map { owner.host.boxRect($0.1) }
        let metrics = PaneDropMetrics(tabEdge: ShellMotion.tabEdgePx, bandMin: ShellMotion.edgeBandMin,
            bandFraction: ShellMotion.edgeBandFraction, bandMaxFraction: ShellMotion.edgeBandMaxFraction,
            gapReach: max(owner.host.bounds.width / owner.host.area.width, owner.host.bounds.height / owner.host.area.height))
        switch PaneDrop.zone(area: owner.host.bounds, panes: rects, source: ids.firstIndex(of: source ?? ""), point: pointer, metrics: metrics) {
        case .centre(let i): setZone(.centre(ids[i]))
        case .paneEdge(let i, let s): setZone(.paneEdge(ids[i], s))
        case .tabEdge(let s): setZone(.tabEdge(s))
        case nil: setZone(nil)
        }
    }
    private func updateDwell(tab: String?) {
        guard phase == .lifted, let tab, tab != owner?.state.selectedTab else { dwell = nil; return }
        if let dwell, dwell.tab == tab, hypot(pointer.x - dwell.point.x, pointer.y - dwell.point.y) < ShellMotion.dragThresholdPx { return }
        dwell = Dwell(tab: tab, point: pointer, frozenStart: frozenMs)
        ensureClock()
    }
    private func dwellElapsed(_ dwell: Dwell) -> CGFloat {
        if let frozenMs { return max(0, frozenMs - (dwell.frozenStart ?? 0)) }
        return CGFloat((ProcessInfo.processInfo.systemUptime - dwell.started) * 1000)
    }
    private func focusSpringTab(_ tab: String) {
        guard let owner else { return }
        sent.append(["method": "tab.focus", "params": ["tab_id": tab]])
        let commands = HerdrCommands(socketPath: socket)
        tabFocusQueue.async { commands.tabFocus(tabId: tab) }
        springSelection = true
        owner.selectTab(tab)
        springSelection = false
    }
    private func evaluateDwell() {
        guard phase == .lifted, let dwell, dwellElapsed(dwell) >= ShellMotion.springLoadMs else { return }
        self.dwell = nil; sprung = true
        setZone(nil)
        focusSpringTab(dwell.tab)
    }
    private func key(_ z: Zone) -> String { String(describing: z) }
    private func params(_ z: Zone, dry: Bool) -> [String: Any]? {
        guard let source else { return nil }
        let target: [String: Any], side: PaneDropSide
        switch z {
        case .paneEdge(let p, let s): target = ["type": "pane", "pane_id": p]; side = s
        case .tabEdge(let s): target = ["type": "tab", "tab_id": owner?.state.selectedTab ?? ""]; side = s
        case .intoTab(let t): target = ["type": "tab", "tab_id": t]; side = .right
        default: return nil
        }
        var out: [String: Any] = ["pane_id": source, "target": target, "side": side.rawValue, "dry_run": dry]
        if !dry { out["focus"] = true }
        return out
    }
    private func setZone(_ value: Zone?) {
        let value = value.flatMap { rejected.contains(key($0)) ? nil : $0 }
        guard zone != value, let owner else { return }
        zone = value
        guard let value else { ghost = nil; ghostTarget = nil; motions.removeAll { $0.kind == "zone" }; draw(); return }
        let ids = owner.host.rects.map { $0.0.paneId }, rects = owner.host.rects.map { owner.host.boxRect($0.1) }
        let local: PaneDropZone?
        switch value {
        case .centre(let p): local = ids.firstIndex(of: p).map { .centre(target: $0) }
        case .paneEdge(let p, let s): local = ids.firstIndex(of: p).map { .paneEdge(target: $0, side: s) }
        case .tabEdge(let s): local = .tabEdge(side: s)
        default: local = nil
        }
        ghostSource = { if case .centre = value { return "target" }; return "estimate" }()
        if let local { retarget(PaneDrop.estimateRect(area: owner.host.bounds, panes: rects, zone: local)) }
        else { ghost = nil; ghostTarget = nil; motions.removeAll { $0.kind == "zone" } }
        if let cached = cache[key(value)] { ghostSource = "placed"; retarget(cached) }
        else { requestDryRun() }
        draw()
    }
    private func requestDryRun() {
        guard phase == .lifted, inFlight == nil, let z = zone, let owner else { return }
        switch z { case .paneEdge, .tabEdge: break; default: return }
        let k = key(z)
        guard cache[k] == nil, !rejected.contains(k), let p = params(z, dry: true) else { return }
        inFlight = k; sent.append(["method": "pane.place", "params": p])
        let gen = generation, layoutGen = layoutGeneration, commands = owner.commands
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let reply = commands.panePlace(params: p)
            DispatchQueue.main.async {
                guard let self, self.generation == gen else { return }
                self.inFlight = nil
                if self.layoutGeneration == layoutGen {
                    let place = (reply?["result"] as? [String: Any])?["place"] as? [String: Any]
                    if place?["changed"] as? Bool == false {
                        self.rejected.insert(k); if self.zone == z { self.setZone(nil) }
                    } else if let r = place?["placed_rect"], let data = try? JSONSerialization.data(withJSONObject: r),
                        let rect = try? JSONDecoder().decode(Snapshot.Rect.self, from: data), let owner = self.owner {
                        let box = owner.host.boxRect(rect); self.cache[k] = box
                        if self.zone == z { self.ghostSource = "placed"; self.retarget(box) }
                    } else { self.rejected.insert(k); if self.zone == z { self.setZone(nil) } }
                    if reply == nil { self.cancel(); return }
                }
                self.requestDryRun(); self.draw()
            }
        }
    }
    func release() {
        guard let owner else { return }
        if phase == .pressed { let p = source; finish(); if let p { owner.focusPane(p) }; return }
        guard phase == .lifted, let z = zone, let source else { cancel(); return }
        motions.removeAll { $0.pane == source && $0.kind == "fade" }
        dwell = nil
        lastZone = z; phase = .dropping
        holdsLayout = true; frozenEpoch = owner.state.selectedTab.map { owner.model.snapshotEpoch(for: $0) } ?? 0
        dropSize = owner.host.bounds.size
        updateSidebarIndicator()
        let method: String, p: [String: Any]
        switch z {
        case .centre(let target): method = "pane.swap"; p = ["source_pane_id": source, "target_pane_id": target]
        case .newTabIn(let ws): method = "pane.move"; p = ["pane_id": source, "destination": ["type": "new_tab", "workspace_id": ws], "focus": true]
        default: method = "pane.place"; p = params(z, dry: false) ?? [:]
        }
        // No pane.focus: pane.swap focuses the source on the server, and pane.place and pane.move carry focus:true.
        sent.append(["method": method, "params": p])
        let commands = owner.commands, gen = generation
        // The drop ends with the server's answer to this call, never with a snapshot: snapshots that land while it
        // is pending are buffered, and the answer settles the panes once from where they stood at the release.
        let send = {
            // No reply in time: restore spring selection (Windows parity), but never roll back pane layout.
            DispatchQueue.main.asyncAfter(deadline: .now() + PaneDrag.replyTimeout) { [weak self] in
                guard let self, self.generation == gen, self.phase == .dropping, self.heldReply?.generation != gen else { return }
                self.restoreAfterFailedDrop(); self.cancel()
            }
            if self.loseNextDrop { self.loseNextDrop = false; return }
            let failTransport = self.failNextDrop
            self.failNextDrop = false
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let reply = failTransport ? nil : commands.paneDragCall(method, p)
                let refused = PaneDrag.refused(reply)
                DispatchQueue.main.async {
                    guard let self else { return }
                    let answer = { [weak self] in
                        guard let self else { return }
                        self.repliesHandled += 1
                        // A drop that already ended (timeout, tab switch, offline, a new drag) ignores its reply entirely.
                        guard self.generation == gen, self.phase == .dropping else { self.repliesIgnored += 1; return }
                        // A refusal (an error, or changed:false such as a pane gone) moved nothing: the buffered snapshot
                        // shows as it is, then the chip goes back.
                        if refused { _ = self.unfreeze(); self.owner?.refreshHost(); self.animateCancel(); return }
                        // With no reply at all the server may still have applied it: end without a rollback.
                        if reply?["result"] == nil { self.restoreAfterFailedDrop(); self.cancel(); return }
                        let buffered = self.unfreeze()
                        self.owner?.applyDropReply(PaneDrag.layouts(reply), buffered: buffered)
                        self.settle()
                        self.owner?.focusPane(source)
                        self.draw()
                    }
                    if self.holdReplies { self.heldReply = (gen, answer) } else { answer() }
                }
            }
        }
        if holdDrops { heldDrop = send } else { send() }
    }
    func sendHeldDrop() { let send = heldDrop; heldDrop = nil; send?() }
    func sendHeldReply() { let held = heldReply; heldReply = nil; held?.answer() }
    /// Ends the freeze; true when any snapshot for the tab landed while it held.
    private func unfreeze() -> Bool {
        guard holdsLayout else { return false }
        holdsLayout = false
        guard let owner, let tab = owner.state.selectedTab else { return false }
        return owner.model.snapshotEpoch(for: tab) != frozenEpoch
    }
    /// The layouts a drop's reply carries: pane.swap's `layout`, or pane.place's and pane.move's target and source.
    private static func layouts(_ reply: [String: Any]?) -> [Snapshot.Layout] {
        guard let result = reply?["result"] as? [String: Any] else { return [] }
        let answer = ["place", "swap", "move_result"].lazy.compactMap { result[$0] as? [String: Any] }.first ?? [:]
        return ["layout", "target_layout", "source_layout"].compactMap { key in
            answer[key].flatMap { try? JSONSerialization.data(withJSONObject: $0) }
                .flatMap { try? JSONDecoder().decode(Snapshot.Layout.self, from: $0) }
        }
    }
    /// The server answered and changed nothing: an error, or `changed: false` in the place, swap or move result.
    private static func refused(_ reply: [String: Any]?) -> Bool {
        guard let reply else { return false }
        guard let result = reply["result"] as? [String: Any] else { return true }
        if result["changed"] as? Bool == false { return true }
        return ["place", "swap", "move_result"].contains { (result[$0] as? [String: Any])?["changed"] as? Bool == false }
    }
    func cancel() {
        guard phase == .pressed || phase == .lifted || phase == .dropping else { return }
        // Once sent, the server may already have committed the drop. Never animate a rollback.
        if phase == .dropping {
            generation += 1; inFlight = nil
            let frozen = holdsLayout
            _ = unfreeze(); finish()
            // The buffered snapshot shows as it is.
            if frozen { owner?.refreshHost() }
            return
        }
        animateCancel()
    }
    private func restoreAfterFailedDrop() {
        if sprung, let originTab, owner?.state.selectedTab != originTab { focusSpringTab(originTab) }
    }
    /// The chip springs back to the source cap. Also ends a drop the server refused, which moved nothing.
    private func animateCancel() {
        dwell = nil
        if sprung, let originTab, owner?.state.selectedTab != originTab {
            focusSpringTab(originTab)
        }
        sprung = false
        lastZone = zone; zone = nil; phase = .cancelling; generation += 1; inFlight = nil
        motions.removeAll { $0.kind == "zone" || $0.kind == "fade" }
        ghost = nil; ghostTarget = nil
        let cap = source.flatMap { owner?.host.capFrames[$0] }
        let target = cap.map { CGRect(x: $0.midX, y: $0.midY, width: 0, height: 0) } ?? .zero
        addMotion(pane: nil, kind: "cancel", from: CGRect(origin: pointer, size: .zero), to: target, duration: ShellMotion.cancelMs)
        draw()
    }
    func selectionChanging(to tab: String?) {
        // A drop still waiting on its reply ends quietly too: its chip and ghost belong to the tab being left.
        if !springSelection, phase == .pressed || phase == .lifted || phase == .dropping,
           tab != owner?.state.selectedTab {
            // A foreign selection owns the next tab. Do not nest an origin selection inside it.
            sprung = false
            cancel()
        }
        if phase == .idle { source = nil }
        DispatchQueue.main.async { [weak self] in self?.probe() }
    }
    func beginKeyboard(pane: String) {
        guard let owner, let cap = owner.host.capFrames[pane] else { return }
        press(pane: pane, at: owner.host.convert(CGPoint(x: cap.midX, y: cap.midY), to: nil))
        guard phase == .pressed else { return }
        keyboard = true; lift(); keyboardTarget = pane
        for side in [PaneDropSide.right, .down, .left, .up] {
            if let p = neighbor(pane, side) { keyboardTarget = p; setZone(.centre(p)); break }
        }
    }
    private func neighbor(_ pane: String, _ side: PaneDropSide) -> String? {
        guard let owner, let r = owner.host.rects.first(where: { $0.0.paneId == pane }).map({ owner.host.boxRect($0.1) }) else { return nil }
        return owner.host.rects.filter { $0.0.paneId != pane }.compactMap { s, rect -> (String, CGFloat)? in
            let b = owner.host.boxRect(rect), dx = b.midX - r.midX, dy = b.midY - r.midY
            switch side {
            case .left: guard dx < 0, b.maxY > r.minY, b.minY < r.maxY else { return nil }
            case .right: guard dx > 0, b.maxY > r.minY, b.minY < r.maxY else { return nil }
            case .up: guard dy < 0, b.maxX > r.minX, b.minX < r.maxX else { return nil }
            case .down: guard dy > 0, b.maxX > r.minX, b.minX < r.maxX else { return nil }
            }
            return (s.paneId, hypot(dx, dy))
        }.min { $0.1 < $1.1 }?.0
    }
    func handleKey(_ e: NSEvent) -> Bool {
        guard phase == .pressed || phase == .lifted else { return false }
        if e.keyCode == 53 { cancel(); return true }
        guard keyboard else { return false }
        if e.keyCode == 36 || e.keyCode == 49 { release(); return true }
        let side: PaneDropSide?
        switch e.keyCode { case 123: side = .left; case 124: side = .right; case 125: side = .down; case 126: side = .up
        default:
            switch e.charactersIgnoringModifiers { case "h": side = .left; case "l": side = .right; case "j": side = .down; case "k": side = .up; default: side = nil }
        }
        if let side, let target = keyboardTarget {
            if e.modifierFlags.contains(.shift) { setZone(target == source ? .tabEdge(side) : .paneEdge(target, side)) }
            else if let next = neighbor(target, side) { keyboardTarget = next; setZone(next == source ? nil : .centre(next)) }
        }
        return true
    }
    func layoutWillApply(old: [String: NSRect], new: [String: NSRect], sameTab: Bool, fromDrop: Bool = false) {
        layoutGeneration += 1; cache = [:]; rejected = []
        guard let owner else { return }
        // A layout landing during an accepted drop's settle retargets it from the boxes drawn last (`old`).
        let retarget = phase == .settling && motions.contains { $0.kind == "settle" || $0.kind == "crossfade" }
        let sameSize = lastHostSize == owner.host.bounds.size
        lastHostSize = owner.host.bounds.size
        // Only an accepted drop settles, once. Snapshots after its settle apply at once.
        let canAnimate = (fromDrop || retarget) && sameTab && sameSize && Set(old.keys) == Set(new.keys) && !owner.resizer.isBusy && !owner.window.inLiveResize
        // Under Reduce Motion a retarget keeps the running fade; the panes just take their new frames.
        if !(canAnimate && retarget && reduce) {
            motions.removeAll { $0.kind == "settle" || $0.kind == "crossfade" }
            if canAnimate {
                for (pane, rect) in new where old[pane] != rect {
                    addMotion(pane: pane, kind: reduce ? "crossfade" : "settle", from: old[pane] ?? rect, to: rect,
                              duration: reduce ? ShellMotion.reducedFadeMs : ShellMotion.settleMs)
                }
            }
        }
        boxes = new
        if phase == .idle && !motions.isEmpty { phase = .settling }
    }
    /// The drop is in: the zone fades out (a crossfade over reducedFadeMs under Reduce Motion) and tick() finishes once
    /// it and any settle are done. Every drop ends here, also one whose layout brings no settle or crossfade, such as a
    /// move into another tab.
    private func settle() {
        phase = .settling
        motions.removeAll { $0.pane == source && $0.kind == "fade" }
        addMotion(pane: nil, kind: "fade", from: .zero, to: .zero, duration: reduce ? ShellMotion.reducedFadeMs : ShellMotion.fadeMs)
    }
    func layoutDidApply() {
        draw()
        if phase == .lifted { requestDryRun() }
    }
    /// The host is about to lay out at `size`. A window resize or a sidebar or side column change while a drop waits on
    /// its reply ends the drop quietly first, so the release-time boxes never rescale and no terminal resizes while
    /// they hold: the buffered snapshot is laid out, at the new size, instead.
    func hostWillLayout(_ size: CGSize) {
        guard phase == .dropping, holdsLayout, size != dropSize else { return }
        cancel()
    }
    func hostSizeChanged() {
        if let owner, owner.host.bounds.size != lastHostSize {
            lastHostSize = owner.host.bounds.size
            motions.removeAll { $0.kind == "settle" || $0.kind == "crossfade" }
            draw()
        }
    }
    func freeze(ms: CGFloat) { frozenMs = ms; link?.isPaused = true; tick() }
    func run() {
        if var dwell = dwell, frozenMs != nil {
            dwell.started = ProcessInfo.processInfo.systemUptime - Double(dwellElapsed(dwell)) / 1000
            dwell.frozenStart = nil; self.dwell = dwell
        }
        frozenMs = nil; link?.isPaused = false; tick()
    }
    private func elapsed(_ m: Motion) -> CGFloat { frozenMs ?? CGFloat((ProcessInfo.processInfo.systemUptime - m.started) * 1000) }
    private func addMotion(pane: String?, kind: String, from: CGRect, to: CGRect, duration: CGFloat) {
        motions.removeAll { $0.pane == pane && $0.kind == kind }
        motions.append(Motion(pane: pane, kind: kind, from: from, to: to, duration: duration))
        ensureClock()
    }
    private func ensureClock() {
        if link == nil, let host = owner?.host {
            link = host.displayLink(target: self, selector: #selector(tick))
            link?.add(to: .main, forMode: .common)
            link?.isPaused = frozenMs != nil
        }
    }
    private func retarget(_ rect: CGRect) {
        ghostTarget = rect
        if reduce || ghost == nil { ghost = rect }
        else { addMotion(pane: nil, kind: "zone", from: ghost ?? rect, to: rect, duration: ShellMotion.zoneMorphMs) }
    }
    private func progress(_ m: Motion) -> CGFloat {
        let t = min(max(elapsed(m) / m.duration, 0), 1)
        let rate = ShellMotion.springDampingFraction / ShellMotion.springResponse
        let value = 1 - (1 + rate * t) * exp(-rate * t), end = 1 - (1 + rate) * exp(-rate)
        return end > 0 ? value / end : t
    }
    private func interpolate(_ a: CGRect, _ b: CGRect, _ t: CGFloat) -> CGRect {
        CGRect(x: a.minX + (b.minX - a.minX) * t, y: a.minY + (b.minY - a.minY) * t,
               width: a.width + (b.width - a.width) * t, height: a.height + (b.height - a.height) * t)
    }
    func advance(ms: CGFloat) {
        frozenMs = ms; link?.isPaused = true; step(prune: true); step(prune: true)
    }
    @objc private func tick() { step(prune: frozenMs == nil) }
    private func step(prune: Bool) {
        evaluateDwell()
        draw()
        if prune {
            // Retain the completed ghost fade until the last moving surface ends. Removing it
            // at fadeMs would paint alpha 1 again for the rest of the 200 ms settle.
            let ended = motions.allSatisfy { elapsed($0) >= $0.duration }
            motions.removeAll { elapsed($0) >= $0.duration && !($0.pane == nil && $0.kind == "fade" && phase == .settling) }
            if ended && phase == .settling { finish(); return }
        }
        if motions.isEmpty && dwell == nil {
            link?.invalidate(); link = nil
            if phase == .cancelling || phase == .settling { finish() }
            else { draw() }
        }
    }
    private func updateSidebarIndicator() {
        var row: String?
        if phase == .lifted {
            switch zone {
            case .intoTab(let id): row = "tab:" + id
            case .newTabIn(let id): row = "space:" + id
            default: break
            }
        }
        if owner?.state.paneDropRow != row { owner?.state.paneDropRow = row }
    }
    private func draw() {
        guard let owner else { return }
        if phase != .lifted { motions.removeAll { $0.pane == source && $0.kind == "fade" } }
        updateSidebarIndicator()
        var drawn = Dictionary(uniqueKeysWithValues: owner.host.rects.map { ($0.0.paneId, owner.host.boxRect($0.1)) })
        for m in motions {
            if m.kind == "zone" { ghost = interpolate(m.from, m.to, progress(m)) }
            if m.kind == "settle", let pane = m.pane { drawn[pane] = interpolate(m.from, m.to, progress(m)) }
            if m.kind == "cancel", !reduce { pointer = interpolate(m.from, m.to, progress(m)).origin }
        }
        boxes = drawn
        let moving = Set(motions.compactMap { ($0.kind == "settle" || $0.kind == "crossfade") ? $0.pane : nil })
        for (pane, box) in drawn {
            let crossfade = motions.first { $0.pane == pane && $0.kind == "crossfade" }
            let alpha = crossfade.map { ease(elapsed($0) / $0.duration) } ?? 1
            owner.host.drawBox(pane, rect: box, opacity: alpha * sourceOpacity(pane), moving: moving.contains(pane))
        }
        overlay.wantsLayer = true
        overlay.layer?.zPosition = 2
        overlay.frame = owner.host.bounds
        owner.host.addSubview(overlay, positioned: .above, relativeTo: nil)
        chipBorder.isHidden = !(phase == .lifted || phase == .dropping || phase == .cancelling)
        overlay.needsDisplay = true
    }
    private func ease(_ value: CGFloat) -> CGFloat {
        let value = min(max(value, 0), 1), points = ShellMotion.ease
        func bezier(_ t: CGFloat, _ a: CGFloat, _ b: CGFloat) -> CGFloat {
            let u = 1 - t
            return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t
        }
        var low: CGFloat = 0, high: CGFloat = 1
        // Invert the token curve's x coordinate; deterministic under the frozen clock.
        for _ in 0..<20 {
            let middle = (low + high) / 2
            if bezier(middle, points[0], points[2]) < value { low = middle } else { high = middle }
        }
        return bezier((low + high) / 2, points[1], points[3])
    }
    private func sourceOpacity(_ pane: String) -> CGFloat {
        guard pane == source else { return 1 }
        let fade = motions.first { $0.pane == source && $0.kind == "fade" }
        let t = fade.map { ease(elapsed($0) / $0.duration) } ?? 1
        if phase == .lifted { return 1 + (ShellMotion.liftOpacity - 1) * t }
        if phase == .cancelling, let m = motions.first(where: { $0.kind == "cancel" }) {
            let t = ease(elapsed(m) / ShellMotion.fadeMs)
            return ShellMotion.liftOpacity + (1 - ShellMotion.liftOpacity) * t
        }
        return 1
    }
    private func resetVisuals() { dwell = nil; sprung = false; motions = []; link?.invalidate(); link = nil; ghost = nil; ghostTarget = nil }
    private func finish() {
        owner?.host.resetDragLayers()
        phase = .idle; zone = nil; heldDrop = nil; resetVisuals()
        if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil
        NSCursor.arrow.set(); draw()
    }
    func dump() -> [String: Any] {
        func rect(_ r: CGRect?) -> Any { r.map { [$0.minX, $0.minY, $0.width, $0.height] } ?? (NSNull() as Any) }
        return ["phase": phase.rawValue, "source": source ?? (NSNull() as Any), "zone": zone?.json ?? (NSNull() as Any),
            "lastZone": lastZone?.json ?? (NSNull() as Any), "ghostRect": rect(ghost), "ghostTarget": rect(ghostTarget),
            "ghostSource": ghostSource, "dryRunPending": inFlight != nil, "placeSupported": placeSupported,
            "ghostAlpha": ghostAlpha,
            "surfaceLayers": owner?.host.rects.map { surface, _ in
                ["pane": surface.paneId, "z": surface.layer?.zPosition ?? 0, "backed": surface.layer?.backgroundColor != nil] as [String: Any]
            } ?? [],
            "sidebarZoneFill": PaneDropFill.drawn(in: owner?.sidebarHostView, row: owner?.state.paneDropRow ?? ""),
            "hairline": ShellFace.ring,
            "chip": ["visible": phase == .lifted || phase == .dropping || phase == .cancelling, "label": label, "text": chipText, "glyph": chipGlyph, "stroke": chipBorder.lineWidth, "sourceState": sourceGlyph.rawValue, "sidebarStatus": sourceGlyph.sidebarStatus,
                     "tone": chipPaintedTone.map { ThemeStore.hex($0) as Any } ?? NSNull()],
            "boxes": boxes.mapValues { [$0.minX, $0.minY, $0.width, $0.height] }, "sent": sent, "sendLog": sendLog,
            "frozen": holdsLayout, "replyHeld": heldReply != nil,
            "replies": ["handled": repliesHandled, "ignored": repliesIgnored],
            "motion": ["frozenMs": frozenMs ?? (NSNull() as Any), "reduce": reduce,
                       "active": motions.map { ["pane": $0.pane ?? (NSNull() as Any), "kind": $0.kind, "elapsedMs": elapsed($0)] }]]
    }
    /// The sidebar row's glyph for the source's state (SpacesTree.statusGlyph), not a set of its own.
    private var chipGlyph: String { SpacesTree.statusGlyph(sourceGlyph.sidebarStatus) }
    private var chipText: String { chipGlyph + " " + label }
    private var zoneFillAlpha: CGFloat {
        owner?.host.tokens.mode == .dark ? ShellMotion.zoneFillAlphaDark : ShellMotion.zoneFillAlphaLight
    }
    private var ghostAlpha: CGFloat {
        guard ghost != nil, phase == .lifted || phase == .dropping || phase == .settling else { return 0 }
        guard phase == .settling, let fade = motions.first(where: { $0.pane == nil && $0.kind == "fade" }) else { return 1 }
        return 1 - ease(elapsed(fade) / fade.duration)
    }
    fileprivate func paint() {
        guard let owner else { return }
        let t = owner.host.tokens
        if phase == .lifted, let source, let box = boxes[source] {
            t.accentNS.setStroke()
            let p = NSBezierPath(roundedRect: box.insetBy(dx: ShellMotion.zoneStroke, dy: ShellMotion.zoneStroke), xRadius: ShellRadius.control, yRadius: ShellRadius.control)
            p.lineWidth = ShellMotion.zoneStroke; p.stroke()
        }
        let ending = motions.first { $0.pane == nil && ($0.kind == "fade" || $0.kind == "cancel") }
        let endingAlpha = ending.map { 1 - ease(elapsed($0) / $0.duration) } ?? 1
        if let ghost, phase == .lifted || phase == .dropping || phase == .settling {
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current?.cgContext.setAlpha(ghostAlpha)
            let p = NSBezierPath(roundedRect: ghost.insetBy(dx: ShellMotion.zoneInset, dy: ShellMotion.zoneInset), xRadius: ShellRadius.control, yRadius: ShellRadius.control)
            t.accentNS.withAlphaComponent(zoneFillAlpha).setFill(); p.fill()
            t.accentNS.setStroke(); p.lineWidth = ShellMotion.zoneStroke; p.stroke()
            NSGraphicsContext.restoreGraphicsState()
        }
        if phase == .lifted || phase == .dropping || phase == .cancelling {
            NSGraphicsContext.saveGraphicsState()
            let fade = motions.first { $0.pane == source && $0.kind == "fade" }
            let alpha = phase == .cancelling ? endingAlpha : fade.map { ease(elapsed($0) / $0.duration) } ?? 1
            NSGraphicsContext.current?.cgContext.setAlpha(alpha)
            let text = chipText
            let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: ShellType.rowTitle), .foregroundColor: t.inkNS]
            let size = (text as NSString).size(withAttributes: attrs)
            let r = CGRect(x: pointer.x + ShellMotion.chipOffset, y: pointer.y + ShellMotion.chipOffset,
                           width: size.width + ShellMotion.chipOffset * 2, height: ShellSpace.paneCapHeight)
            NSColor(hex: t.chrome.panel).setFill(); NSBezierPath(roundedRect: r, xRadius: ShellRadius.control, yRadius: ShellRadius.control).fill()
            CATransaction.begin(); CATransaction.setDisableActions(true)
            chipBorder.path = CGPath(roundedRect: r, cornerWidth: ShellRadius.control, cornerHeight: ShellRadius.control, transform: nil)
            chipBorder.fillColor = nil
            chipBorder.strokeColor = NSColor(hex: t.chrome.line).cgColor
            chipBorder.lineWidth = ShellFace.ring
            chipBorder.opacity = Float(alpha)
            if chipBorder.superlayer == nil { overlay.layer?.addSublayer(chipBorder) }
            CATransaction.commit()
            let attributed = NSMutableAttributedString(string: text, attributes: attrs)
            let tone = t.stateTone(sourceGlyph.sidebarStatus)
            chipPaintedTone = tone
            attributed.addAttribute(.foregroundColor, value: NSColor(hex: tone), range: NSRange(location: 0, length: (chipGlyph as NSString).length))
            attributed.draw(at: CGPoint(x: r.minX + ShellMotion.chipOffset, y: r.midY - size.height / 2))
            NSGraphicsContext.restoreGraphicsState()
        }
    }
}
private final class PaneDragOverlay: NSView {
    weak var drag: PaneDrag?
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func draw(_ dirtyRect: NSRect) { drag?.paint() }
}
