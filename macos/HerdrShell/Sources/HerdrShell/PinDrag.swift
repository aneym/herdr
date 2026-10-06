import AppKit
import SwiftUI

/// Drag a PINNED row up or down to a new place in its machine's pin order.
///
/// The order is a herdr fact (`tab.pin_move`), so a drop sends the move to the machine that owns
/// the chat and every client follows from that machine's next snapshot. Until the snapshot shows
/// it, the new order stands here as `pending`, so the rows settle at once and ⌘1..9 follow it.
/// Pins move within their own machine: the section lists this Mac's pins, then each machine's,
/// and that block order is the same on every surface.
final class PinDrag: ObservableObject {
    static let shared = PinDrag()

    /// Pointer travel before a press on a pinned row becomes a drag; less is a click.
    static let threshold: CGFloat = 4
    /// How long a dropped order waits for its machine before the snapshot order shows again.
    static let pendingLifetime: TimeInterval = 4

    /// Row id being dragged ("pinned:<tab>"), nil when no drag runs.
    @Published private(set) var dragged: String?
    /// Pointer travel since the press, in sidebar points.
    @Published private(set) var travel: CGFloat = 0
    /// Slot in the dragged row's machine block it would land in; nil off the section.
    @Published private(set) var target: Int?
    /// Dropped orders, by machine ("" is this Mac), as tab ids, until their snapshot agrees.
    @Published private(set) var pending: [String: (order: [String], at: Date)] = [:]

    /// The dragged row's machine block at the press: row ids and their frames, top to bottom.
    private var block: [(id: String, frame: CGRect)] = []
    private var cancelled = false
    private var escMonitor: Any?

    static func machine(of tab: String) -> String { Machines.split(tab)?.machine ?? "" }

    // MARK: Gesture

    func changed(_ row: SpacesRow, rows: [SpacesRow], frames: [String: CGRect], location: CGPoint, start: CGPoint) {
        if cancelled { return }
        if dragged == nil {
            guard let tab = row.tab else { return }
            let machine = Self.machine(of: tab)
            block = rows.filter { $0.id.hasPrefix("pinned:") && Self.machine(of: $0.tab ?? "") == machine }
                .compactMap { r in frames[r.id].map { (r.id, $0) } }
            guard block.count > 1, block.contains(where: { $0.id == row.id }) else { block = []; return }
            dragged = row.id
            escMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard event.keyCode == 53, let self, self.dragged != nil else { return event }
                self.cancel()
                return nil
            }
        }
        travel = location.y - start.y
        target = slot(at: location)
    }

    /// Release: a drop on the section moves the pin; off the section, or after Esc, nothing moves.
    func ended(model: HerdrModel) {
        defer { cancelled = false }
        guard !cancelled, let dragged, let target, let from = block.firstIndex(where: { $0.id == dragged }) else {
            cancel()
            return
        }
        guard target != from else { cancel(); return }
        let ids = block.compactMap { tabId(fromRow: $0.id) }
        commit(model: model, ids: ids, from: from, to: target)
        withAnimation(.easeOut(duration: 0.18)) { reset() }
    }

    func cancel() {
        cancelled = dragged != nil
        withAnimation(.easeOut(duration: 0.18)) { reset() }
    }

    private func reset() {
        if let escMonitor { NSEvent.removeMonitor(escMonitor) }
        escMonitor = nil
        dragged = nil
        travel = 0
        target = nil
        block = []
    }

    /// The slot under the pointer: a block row's own slot, the first or last one just past the
    /// block's ends, nil more than a row beyond them or outside the sidebar's columns.
    private func slot(at p: CGPoint) -> Int? {
        guard let first = block.first?.frame, let last = block.last?.frame else { return nil }
        let row = first.height
        guard p.x >= first.minX - row, p.x <= first.maxX + row,
              p.y >= first.minY - row, p.y <= last.maxY + row else { return nil }
        if p.y < first.minY { return 0 }
        return block.firstIndex { p.y < $0.frame.maxY } ?? block.count - 1
    }

    // MARK: Drop

    private func tabId(fromRow id: String) -> String? {
        id.hasPrefix("pinned:") ? String(id.dropFirst("pinned:".count)) : nil
    }

    /// Moves `ids[from]` to slot `to` on its machine and shows the new order until it lands.
    func commit(model: HerdrModel, ids: [String], from: Int, to: Int) {
        guard ids.indices.contains(from), ids.indices.contains(to), from != to else { return }
        let tab = ids[from]
        // The server counts its whole pin list; the chat now in that slot names the index.
        guard let pinIndex = model.source(for: ids[to])?.tabs.first(where: { $0.tab_id == ids[to] })?.pin_index else { return }
        var order = ids
        order.remove(at: from)
        order.insert(tab, at: to)
        withAnimation(.easeOut(duration: 0.18)) { pending[Self.machine(of: tab)] = (order, Date()) }
        let commands = HerdrCommands(socketPath: model.env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async {
            let ok = commands.tabPinMove(tabId: tab, pinIndex: pinIndex)
            if !ok { DispatchQueue.main.async { withAnimation { self.pending[Self.machine(of: tab)] = nil } } }
        }
    }

    // MARK: Pending order

    /// `ids` (one machine's pins in snapshot order) in the dropped order while that stands.
    func ordered(_ ids: [String]) -> [String] {
        guard let first = ids.first, let entry = pending[Self.machine(of: first)],
              Date().timeIntervalSince(entry.at) < Self.pendingLifetime,
              Set(entry.order) == Set(ids) else { return ids }
        if entry.order == ids {
            // The machine has caught up; drop the stand-in after this pass.
            let machine = Self.machine(of: first)
            DispatchQueue.main.async { if self.pending[machine]?.order == ids { self.pending[machine] = nil } }
        }
        return entry.order
    }

    /// Reorders each machine's PINNED rows in place to its dropped order.
    func reorder(_ rows: [SpacesRow]) -> [SpacesRow] {
        guard !pending.isEmpty else { return rows }
        var out = rows
        let slots = rows.indices.filter { rows[$0].id.hasPrefix("pinned:") }
        for machine in Set(slots.map { Self.machine(of: rows[$0].tab ?? "") }) {
            let mine = slots.filter { Self.machine(of: rows[$0].tab ?? "") == machine }
            let ids = mine.compactMap { rows[$0].tab }
            let order = ordered(ids)
            guard order != ids else { continue }
            let byTab = Dictionary(mine.map { (rows[$0].tab ?? "", rows[$0]) }, uniquingKeysWith: { a, _ in a })
            for (slot, tab) in zip(mine, order) { if let row = byTab[tab] { out[slot] = row } }
        }
        return out
    }
}

extension View {
    /// A PINNED row the pointer can drag to a new place among its machine's pins. A press that
    /// moves less than `PinDrag.threshold` stays the row's click.
    func pinDraggable(_ row: SpacesRow, rows: [SpacesRow], frames: @escaping () -> [String: CGRect], drag: PinDrag,
                      model: HerdrModel, t: Tokens) -> some View {
        modifier(PinDragRow(row: row, rows: rows, frames: frames, drag: drag, model: model, t: t))
    }
}

private struct PinDragRow: ViewModifier {
    let row: SpacesRow
    let rows: [SpacesRow]
    let frames: () -> [String: CGRect]
    @ObservedObject var drag: PinDrag
    let model: HerdrModel
    let t: Tokens

    func body(content: Content) -> some View {
        let isDragged = drag.dragged == row.id
        let line = insertion
        return content
            .overlay(alignment: line == .above ? .top : .bottom) {
                if line != nil {
                    Rectangle().fill(t.mute.opacity(0.7)).frame(height: 1.5)
                        .offset(y: line == .above ? -1 : 1).allowsHitTesting(false)
                }
            }
            .offset(y: isDragged ? drag.travel : 0)
            .opacity(isDragged ? 0.85 : 1)
            .zIndex(isDragged ? 1 : 0)
            .highPriorityGesture(
                DragGesture(minimumDistance: PinDrag.threshold, coordinateSpace: .named("click"))
                    .onChanged { value in
                        drag.changed(row, rows: rows, frames: frames(), location: value.location, start: value.startLocation)
                    }
                    .onEnded { _ in drag.ended(model: model) },
                // Only PINNED rows drag; every other row keeps its gestures as they were.
                including: row.id.hasPrefix("pinned:") ? .all : .subviews
            )
    }

    private enum Edge { case above, below }

    /// Where the insertion line sits on this row: above the slot when the pin moves up, below it
    /// when it moves down, nowhere for the pin's own slot.
    private var insertion: Edge? {
        guard let dragged = drag.dragged, let target = drag.target, dragged != row.id else { return nil }
        let block = rows.filter { $0.id.hasPrefix("pinned:") && PinDrag.machine(of: $0.tab ?? "") == PinDrag.machine(of: row.tab ?? "") }
        guard let from = block.firstIndex(where: { $0.id == dragged }),
              let mine = block.firstIndex(where: { $0.id == row.id }), mine == target, target != from else { return nil }
        return target < from ? .above : .below
    }
}
