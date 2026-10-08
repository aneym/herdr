import AppKit
import SwiftUI

// P18: one scrolling page. Every string and color choice arrives already decided.

struct FactoryPage: View {
    @ObservedObject var model: FactoryModel
    @ObservedObject var theme: ThemeStore
    var body: some View {
        FactoryView(snapshot: model.snapshot, tokens: theme.tokens, scrolls: true,
                    toggleRoute: { model.toggleRoute($0) }, openRouting: { model.openRoutingTable() })
    }
}

struct FactoryView: View {
    var snapshot: FactorySnapshot
    var tokens: Tokens
    var scrolls = true
    var toggleRoute: (String) -> Void
    var openRouting: () -> Void

    private var t: Tokens { tokens }

    var body: some View {
        let stack = VStack(alignment: .leading, spacing: ShellSpace.step22) {
            header
            machines
            pools
            routing
            inFlight
        }
        .padding(ShellSpace.step20)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        Group {
            if scrolls {
                ScrollView { stack }
            } else {
                stack
            }
        }
        .font(.system(size: ShellType.size12, design: .monospaced))
        .foregroundStyle(t.ink)
        .background(t.windowBg)
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            Text("Factory").font(.system(size: ShellType.size18, weight: .semibold))
            Spacer(minLength: 8)
            Text(snapshot.updated).foregroundStyle(t.mute).monospacedDigit()
        }
    }

    private var machines: some View {
        section("Machines") {
            if snapshot.machines.isEmpty { Text("no machines").foregroundStyle(t.mute) }
            ForEach(snapshot.machines) { row in
                machineRow(row)
            }
        }
    }

    private var pools: some View {
        section(poolsTitle) {
            if snapshot.pools.isEmpty {
                Text(snapshot.poolsStale ? "no pool data" : "waiting for pools").foregroundStyle(t.mute)
            }
            ForEach(snapshot.pools) { row in
                poolRow(row)
            }
        }
    }

    private var poolsTitle: String {
        snapshot.poolsAge.isEmpty ? "Pools" : "Pools · \(snapshot.poolsAge)"
    }

    private var routing: some View {
        section("Routing · ladder: \(snapshot.ladderMode.isEmpty ? "—" : snapshot.ladderMode)") {
            if snapshot.routes.isEmpty { Text("no ladder").foregroundStyle(t.mute) }
            ForEach(snapshot.routes) { row in
                routeRow(row)
            }
            if !snapshot.decider.isEmpty {
                Text(snapshot.decider).foregroundStyle(t.mute).font(.system(size: ShellType.switcherMeta)).padding(.top, ShellSpace.step4)
            }
            Button(action: openRouting) {
                Text("Open routing table")
                    .padding(.horizontal, ShellSpace.step8).padding(.vertical, ShellSpace.step4)
                    .overlay(RoundedRectangle(cornerRadius: ShellRadius.curve5).stroke(t.line, lineWidth: 1))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.top, ShellSpace.step4)
        }
    }

    private var inFlight: some View {
        section("In flight") {
            if snapshot.flights.isEmpty { Text("nothing live").foregroundStyle(t.mute) }
            ForEach(snapshot.flights) { row in
                flightRow(row)
            }
            VStack(alignment: .leading, spacing: ShellSpace.step4) {
                Text("Landed today: \(snapshot.landedCount)").foregroundStyle(t.mute).padding(.top, ShellSpace.step6)
                ForEach(snapshot.landed) { row in
                    HStack(alignment: .firstTextBaseline, spacing: ShellSpace.step8) {
                        Text(row.time).foregroundStyle(t.mute).monospacedDigit().frame(width: 72, alignment: .trailing)
                        Text(row.subject).lineLimit(1)
                    }
                }
            }
        }
    }

    private func section<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: ShellSpace.step8) {
            Text(title)
                .font(.system(size: ShellType.switcherRow, weight: .semibold))
                .foregroundStyle(t.ink)
            Rectangle().fill(t.line).frame(height: 1)
            content()
        }
    }

    private func machineRow(_ row: MachineRow) -> some View {
        VStack(alignment: .leading, spacing: ShellSpace.step2) {
            HStack(alignment: .firstTextBaseline, spacing: ShellSpace.step8) {
                if !row.usageState.isEmpty {
                    StateGlyph(state: usageGlyph(row.usageState), tokens: t, scale: 0.8)
                }
                Text(row.name).foregroundStyle(attention(row.attention)).lineLimit(1)
                if !row.kind.isEmpty {
                    Text(row.kind).foregroundStyle(t.mute).font(.system(size: ShellType.switcherMeta))
                }
                Text(row.summary).foregroundStyle(row.attention.isEmpty ? t.mute : attention(row.attention)).lineLimit(1)
                Spacer(minLength: 8)
                Text(row.state).foregroundStyle(stateColor(row.state)).monospacedDigit().lineLimit(1)
            }
            HStack(spacing: ShellSpace.step8) {
                if !row.usageLine.isEmpty {
                    Text(row.usageLine).foregroundStyle(usageColor(row.usageState)).monospacedDigit().lineLimit(1)
                }
                if !row.slots.isEmpty {
                    Text(row.slots).monospacedDigit().frame(width: 64, alignment: .trailing)
                }
                Text(row.disk).lineLimit(1)
                Spacer(minLength: 0)
            }
            .font(.system(size: ShellType.switcherMeta))
            .foregroundStyle(t.mute)
        }
        .opacity(row.dimmed ? 0.4 : 1)
        .padding(.vertical, ShellSpace.step2)
    }

    private func poolRow(_ row: PoolRow) -> some View {
        let color = tone(row.tone)
        return VStack(alignment: .leading, spacing: ShellSpace.step4) {
            HStack(alignment: .firstTextBaseline, spacing: ShellSpace.step8) {
                Text(row.provider).foregroundStyle(color)
                Text(row.id).foregroundStyle(color).lineLimit(1)
                Spacer(minLength: 8)
                Text(row.counts).foregroundStyle(color).monospacedDigit()
            }
            barLine("5h", fraction: row.fiveHour, label: row.fiveHourLabel, color: color)
            barLine("wk", fraction: row.weekly, label: row.weeklyLabel, color: color)
            if !row.pace.isEmpty || !row.monthly.isEmpty || !row.refill.isEmpty {
                HStack(spacing: ShellSpace.step12) {
                    if !row.pace.isEmpty { Text(row.pace).monospacedDigit() }
                    if !row.monthly.isEmpty { Text(row.monthly).monospacedDigit().lineLimit(1) }
                    Spacer(minLength: 4)
                    if !row.refill.isEmpty { Text(row.refill).monospacedDigit() }
                }
                .font(.system(size: ShellType.switcherMeta))
                .foregroundStyle(t.mute)
            }
        }
        .padding(.vertical, ShellSpace.step3)
    }

    private func barLine(_ name: String, fraction: Double?, label: String, color: Color) -> some View {
        HStack(spacing: ShellSpace.step8) {
            Text(name).font(.system(size: ShellType.switcherMeta)).foregroundStyle(t.mute).frame(width: 22, alignment: .leading)
            ThinBar(percent: fraction, fill: color, track: t.line)
                .frame(width: 140)
            Text(label.isEmpty ? "—" : label)
                .font(.system(size: ShellType.switcherMeta))
                .foregroundStyle(t.mute)
                .monospacedDigit()
                .lineLimit(1)
            Spacer(minLength: 0)
        }
    }

    private func routeRow(_ row: RouteRow) -> some View {
        VStack(alignment: .leading, spacing: ShellSpace.step6) {
            Button { toggleRoute(row.name) } label: {
                HStack(alignment: .firstTextBaseline, spacing: ShellSpace.step8) {
                    Text(row.expanded ? "▾" : "▸").foregroundStyle(t.mute).frame(width: 12)
                    Text(row.name).frame(width: 96, alignment: .leading)
                    ChipFlow(chips: row.chips, tokens: t)
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if row.expanded, !row.pick.isEmpty {
                Text(row.pick)
                    .font(.system(size: ShellType.switcherMeta))
                    .foregroundStyle(t.mute)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.leading, ShellSpace.step20)
            } else if row.expanded {
                Text("asking route pick…").font(.system(size: ShellType.switcherMeta)).foregroundStyle(t.mute).padding(.leading, ShellSpace.step20)
            }
        }
    }

    private func flightRow(_ row: FlightRow) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: ShellSpace.step8) {
            Text(row.name).lineLimit(1)
            Text(row.lane).foregroundStyle(t.lane).lineLimit(1)
            Text(row.tab).foregroundStyle(t.mute).lineLimit(1)
            if !row.host.isEmpty {
                Text(row.host).foregroundStyle(t.mute)
            }
            if row.headless {
                Text("headless")
                    .font(.system(size: ShellType.glyph, weight: .semibold))
                    .padding(.horizontal, ShellSpace.step5).padding(.vertical, 1)
                    .overlay(RoundedRectangle(cornerRadius: ShellRadius.row).stroke(t.line, lineWidth: 1))
            }
            Spacer(minLength: 4)
            Text(row.age).foregroundStyle(t.mute).monospacedDigit()
        }
    }

    private func usageGlyph(_ state: String) -> ShellState {
        switch state {
        case "overloaded": return .blocked
        case "idle": return .idle
        default: return .working
        }
    }

    private func usageColor(_ state: String) -> Color {
        switch state {
        case "overloaded": return t.bad
        case "idle": return t.mute
        default: return t.ok
        }
    }

    private func attention(_ value: String) -> Color {
        switch value {
        case "warn": return t.warn
        case "act": return act
        default: return t.ink
        }
    }

    private func stateColor(_ state: String) -> Color {
        if state.hasPrefix("drained") || state.hasPrefix("down") || state == "held" { return t.warn }
        return t.ok
    }

    private func tone(_ value: String) -> Color {
        switch value {
        case "amber": return t.warn
        case "red": return act
        default: return t.ink
        }
    }

    private var act: Color { t.attentionAct }
}

struct ThinBar: View {
    var percent: Double?
    var fill: Color
    var track: Color

    var body: some View {
        GeometryReader { geo in
            let width = geo.size.width * CGFloat(min(100, max(0, percent ?? 0))) / 100
            ZStack(alignment: .leading) {
                Capsule().fill(track)
                Capsule().fill(fill).frame(width: max(0, width))
            }
        }
        .frame(height: 4)
    }
}

struct ChipFlow: View {
    var chips: [String]
    var tokens: Tokens

    var body: some View {
        Flow(spacing: ShellSpace.step4) {
            ForEach(Array(chips.enumerated()), id: \.offset) { _, chip in
                Text(chip)
                    .font(.system(size: ShellType.sectionLabel, design: .monospaced))
                    .padding(.horizontal, ShellSpace.step6).padding(.vertical, ShellSpace.step2)
                    .background(RoundedRectangle(cornerRadius: ShellRadius.row).fill(tokens.sel))
                    .overlay(RoundedRectangle(cornerRadius: ShellRadius.row).stroke(tokens.line, lineWidth: 1))
            }
        }
    }
}

struct Flow: Layout {
    var spacing: CGFloat = ShellSpace.step4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxW = proposal.width ?? 640
        var x: CGFloat = 0
        var y: CGFloat = 0
        var row: CGFloat = 0
        for sub in subviews {
            let size = sub.sizeThatFits(.unspecified)
            if x > 0, x + size.width > maxW {
                x = 0
                y += row + spacing
                row = 0
            }
            row = max(row, size.height)
            x += size.width + spacing
        }
        return CGSize(width: maxW, height: y + row)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var row: CGFloat = 0
        for sub in subviews {
            let size = sub.sizeThatFits(.unspecified)
            if x > bounds.minX, x + size.width > bounds.maxX {
                x = bounds.minX
                y += row + spacing
                row = 0
            }
            sub.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: size.width, height: size.height))
            x += size.width + spacing
            row = max(row, size.height)
        }
    }
}

enum FactoryDemoUI {
    static func hosting(model: FactoryModel, theme: ThemeStore) -> NSView {
        NSHostingView(rootView: FactoryScreen(model: model, theme: theme))
    }
}

private struct FactoryScreen: View {
    @ObservedObject var model: FactoryModel
    @ObservedObject var theme: ThemeStore

    var body: some View {
        FactoryView(snapshot: model.snapshot, tokens: theme.tokens,
                    toggleRoute: { model.toggleRoute($0) },
                    openRouting: { model.openRoutingTable() })
    }
}
enum FactoryShot {
    @MainActor static func write(_ snapshot: FactorySnapshot, tokens: Tokens, to path: String) -> Bool {
        let view = FactoryView(snapshot: snapshot, tokens: tokens, scrolls: false, toggleRoute: { _ in }, openRouting: {})
            .frame(width: 1100, height: 2800, alignment: .topLeading)
            .environment(\.colorScheme, tokens.mode == .dark ? .dark : .light)
        let renderer = ImageRenderer(content: view)
        renderer.scale = 2
        guard let image = renderer.cgImage else { return false }
        let rep = NSBitmapImageRep(cgImage: image)
        guard let data = rep.representation(using: .png, properties: [:]) else { return false }
        return (try? data.write(to: URL(fileURLWithPath: path))) != nil
    }
}
