import SwiftUI

/// One Spaces sidebar row, as Ghostty's tree draws it.
///
/// Columns have fixed widths, so chevrons, status glyphs, counts and the "!" line up
/// down the sidebar whatever a title says. The title takes the width that is left and
/// ends in an ellipsis; trailing text truncates too and never sizes itself past the
/// column, so one long label can't widen the sidebar and shift every other row.
struct SpacesRowView: View {
    let row: SpacesRow
    let t: Tokens
    var firstSpaceId: String? = nil
    var selected = false
    /// Section focus toggle ("◎" or "✕"); nil when the section has none.
    var focusMark: String? = nil
    var pinned = false
    /// The pointer is over this row. Pinned-section rows show their unpin toggle only then.
    var hovered = false
    var showResume = false
    var doc: TabDoc? = nil
    /// The goal row's filter menu; the sidebar owns its choices.
    var goal: AnyView? = nil
    var collapseSpaces: AnyView? = nil
    var click: (String) -> Void = { _ in }
    var resume: () -> Void = {}

    static let chevronWidth: CGFloat = 9
    /// One slot for a state glyph or an agent face, so AGENTS and PINNED titles line up.
    static let glyphWidth: CGFloat = ShellFace.size
    static let alertWidth: CGFloat = 7
    /// Hit width of a trailing pin, + or chevron.
    static let iconHitWidth: CGFloat = ShellSpace.step16
    /// Trailing text is at least as wide as a three-digit count, so "3" and "12" rows
    /// give their titles the same room.
    static let countWidth: CGFloat = 20
    /// A title keeps this much before trailing text gives way.
    static let titleMinWidth: CGFloat = 96

    private var isFooter: Bool { row.kind == .footerUsage || row.kind == .footerHost }
    /// Tab, section and Hidden-agent header rows hold the leading chevron slot, so their
    /// labels share one column. The Hidden header keeps its actual chevron trailing.
    /// Runs never open; their indent already nests them.
    private var treeRow: Bool { row.kind == .tab || row.kind == .section || row.id == "hiddenagents" }
    private var agentRow: Bool { row.id.hasPrefix("agent:") || row.id.hasPrefix("hiddenagent:") }

    var body: some View {
        HStack(spacing: ShellSpace.sidebarRowGap) {
            if row.chevron != "none", row.kind != .space, row.kind != .hidden {
                chevron.frame(maxHeight: .infinity).contentShape(Rectangle())
                    .onTapGesture { click("chevron") }.clickTarget("chevron:" + row.id)
            } else if treeRow {
                Color.clear.frame(width: Self.chevronWidth, height: 1)
            }
            if let face = row.face {
                AgentFace(face: face, dot: row.faceDot, tone: row.tone, request: row.request, t: t).clickTarget("face:" + row.id)
            } else if !row.glyph.isEmpty {
                Text(row.glyph).font(.system(size: ShellType.glyph)).foregroundStyle(tone).frame(width: Self.glyphWidth)
            }
            if row.kind == .goal {
                Text("goal").foregroundStyle(t.mute)
                goal
            } else {
                Text(row.title)
                    .font(.system(size: row.kind == .section ? ShellType.sectionLabel : ShellType.rowTitle,
                                  weight: row.kind == .space || row.kind == .title ? .semibold
                                      : (row.kind == .tab && !row.dim ? .medium : .regular)))
                    .tracking(row.kind == .section ? ShellType.sectionTracking : 0)
                    .foregroundStyle(row.kind == .section || row.kind == .group || row.kind == .hidden || row.dim ? t.mute : t.ink)
                    .lineLimit(1).truncationMode(.tail)
                    .frame(minWidth: isFooter ? nil : Self.titleMinWidth, alignment: .leading)
                    // A footer's host name stays whole; its long summary is what gives way.
                    .layoutPriority(isFooter ? 2 : 0)
            }
            if let request = row.trailingRequest {
                Circle().fill(t.accent).frame(width: ShellFace.dot, height: ShellFace.dot)
                    .help("request \(request)")
            }
            if !agentRow, let machine = row.badge { badge(machine) }
            Spacer(minLength: 4)
            if row.kind == .goal { collapseSpaces }
            if !row.trailing.isEmpty, row.kind != .goal {
                trailingText.font(.system(size: ShellType.sectionLabel)).monospacedDigit().foregroundStyle(row.link == nil ? t.mute : t.accent)
                    .lineLimit(1).truncationMode(.tail)
                    .frame(minWidth: Self.countWidth, alignment: .trailing)
                    .layoutPriority(1)
                    .onTapGesture { click(row.link == nil ? "body" : "link") }
            }
            if agentRow, let place = row.place {
                Text(place).font(.system(size: ShellType.sectionLabel)).foregroundStyle(t.mute)
                    .lineLimit(1).fixedSize()
                    .help("Runs on " + place).accessibilityLabel("Runs on " + place)
            }
            if let home = row.home, let glyph = homeGlyph(home) {
                Text(glyph).font(.system(size: ShellType.rowTrailing))
                    .foregroundStyle(home == "unsynced" ? t.warn : t.mute)
                    .help(homeLabel(home)).accessibilityLabel(homeLabel(home))
            }
            // Agent machine badges follow the home glyph (spec line 98).
            if agentRow, let machine = row.badge { badge(machine) }
            if row.id == "hiddenagents" {
                if row.alert == "act" {
                    Circle().fill(t.accent).frame(width: ShellFace.dot, height: ShellFace.dot)
                        .accessibilityLabel("Hidden agent needs attention")
                }
            } else if row.kind != .goal, !isFooter, row.kind != .title {
                Text(row.alert == "none" ? "" : "!").fontWeight(.bold)
                    .foregroundStyle(row.alert == "act" ? t.bad : t.warn)
                    .frame(width: Self.alertWidth)
            } else if row.alert != "none" {
                Text("!").fontWeight(.bold).foregroundStyle(row.alert == "act" ? t.bad : t.warn)
            }
            if showResume {
                Button("Resume") { resume() }.buttonStyle(.plain).foregroundStyle(t.accent).fixedSize()
            }
            if row.id == "pinned" {
                // A new chat pinned at the end of this section (HerdrModel.newPinnedTab).
                Text("+").foregroundStyle(t.mute).fixedSize().onTapGesture { click("plus") }.clickTarget("pinned+")
            }
            if row.kind == .section {
                Text(focusMark ?? "").font(.system(size: ShellType.glyph)).foregroundStyle(t.mute)
                    .frame(width: 10).onTapGesture { if focusMark != nil { click("focus") } }
            }
            // Rows at rest show name, dot and icons only, as the mock and the Windows Shell: a tab's pin
            // and a space's + come on hover, and a pinned space keeps its accent pin.
            if hovered || pinned, row.tab != nil, let doc {
                docIcon(doc)
            }
            if row.kind == .tab, hovered {
                icon("⚲", "pin", tone: pinned ? t.accent : t.mute)
            }
            if row.kind == .space {
                if hovered || pinned {
                    icon("⚲", "pin", tone: pinned ? t.accent : t.mute)
                }
                if hovered {
                    icon("+", "plus", tone: t.mute)
                }
            }
            if row.kind == .space || row.kind == .hidden, row.chevron != "none" {
                chevron.frame(minWidth: Self.iconHitWidth, maxHeight: .infinity).contentShape(Rectangle())
                    .onTapGesture { click("chevron") }.clickTarget("chevron:" + row.id)
            }
        }
        .padding(.top, row.kind == .space && row.id != firstSpaceId ? ShellSpace.step10 : 0)
        .frame(height: ShellSpace.rowHeight)
        .padding(.leading, indent).padding(.horizontal, ShellSpace.sidebarRowInset)
        .background(RoundedRectangle(cornerRadius: ShellRadius.row).fill(selected && row.kind == .tab ? t.sel : .clear))
    }

    private func homeGlyph(_ home: String) -> String? {
        switch home {
        case "cloud": return "☁︎"
        case "local": return "⌂︎"
        case "unsynced": return "⇡︎"
        default: return nil
        }
    }
    private func homeLabel(_ home: String) -> String {
        switch home {
        case "cloud": return "Memory in Rails cloud"
        case "local": return "Memory on this machine only"
        default: return "Memory not synced to Rails cloud"
        }
    }

    /// Host summaries shed whole trailing fields before the first field clips.
    /// Fixed-size candidates make ViewThatFits measure their untruncated width.
    @ViewBuilder private var trailingText: some View {
        if row.kind == .footerHost {
            let fields = row.trailing.components(separatedBy: " · ")
            ViewThatFits(in: .horizontal) {
                Text(fields.joined(separator: " ")).fixedSize()
                ForEach(Array((1..<fields.count).reversed()), id: \.self) { count in
                    Text(fields.prefix(count).joined(separator: " ") + " …").fixedSize()
                }
                Text((fields.first ?? "") + (fields.count > 1 ? " …" : ""))
            }
        } else {
            Text(row.trailing)
        }
    }

    /// The machine running this chat: an icon and its short name, quiet after the title. An
    /// unreachable machine or one that needs an update dims it further and says so on hover.
    private func badge(_ machine: String) -> some View {
        HStack(spacing: ShellSpace.step2) {
            Image(systemName: "desktopcomputer").font(.system(size: ShellType.size8_5))
            Text(machine).font(.system(size: ShellType.sectionLabel))
        }
        .foregroundStyle(t.mute)
        .opacity(row.badgeState == nil ? 0.9 : 0.45)
        .lineLimit(1).fixedSize()
        .layoutPriority(1)
        .help(row.badgeState.map { machine + ": " + $0 } ?? "Running on " + machine)
    }

    /// A trailing glyph control. Its hit area is the row's full height and wider than the glyph,
    /// so the pin, + and chevron are each easy to hit and never share a pixel.
    private func icon(_ glyph: String, _ part: String, tone: Color) -> some View {
        Text(glyph).foregroundStyle(tone).fixedSize()
            .frame(minWidth: Self.iconHitWidth, maxHeight: .infinity).contentShape(Rectangle())
            .onTapGesture { click(part) }.clickTarget(part + ":" + row.id)
    }

    private func docIcon(_ doc: TabDoc) -> some View {
        Image(systemName: "doc.text").font(.system(size: ShellType.glyph)).foregroundStyle(t.mute)
            .frame(minWidth: Self.iconHitWidth, maxHeight: .infinity).contentShape(Rectangle())
            .onTapGesture { click("doc") }.clickTarget("doc:" + row.id)
            .help("Open " + doc.title + " in Aside").accessibilityLabel("Open review doc " + doc.title)
    }

    private var chevron: some View {
        Image(systemName: row.chevron == "open" ? "chevron.down" : "chevron.right").font(.system(size: ShellType.size8, weight: .semibold)).foregroundStyle(t.mute)
            .frame(width: Self.chevronWidth).onTapGesture { click("chevron") }
    }

    /// Status color, as Ghostty: working green, blocked red, done peach, idle and unknown mute.
    private var tone: Color { Color(hex: t.stateTone(row.tone)) }

    /// Leading inset: sections sit under the space name, rows under the section label.
    private var indent: CGFloat {
        switch row.kind {
        case .tab, .run: return CGFloat(row.depth) * 12 + 12
        case .section, .group: return CGFloat(row.depth) * 12
        default: return 0
        }
    }
}
