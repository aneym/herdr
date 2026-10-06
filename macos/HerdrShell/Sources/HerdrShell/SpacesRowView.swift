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
    var showResume = false
    /// The goal row's filter menu; the sidebar owns its choices.
    var goal: AnyView? = nil
    var click: (String) -> Void = { _ in }
    var resume: () -> Void = {}

    static let chevronWidth: CGFloat = 9
    static let glyphWidth: CGFloat = 12
    static let alertWidth: CGFloat = 7
    /// Trailing text is at least as wide as a three-digit count, so "3" and "12" rows
    /// give their titles the same room.
    static let countWidth: CGFloat = 20
    /// A title keeps this much before trailing text gives way.
    static let titleMinWidth: CGFloat = 96

    private var isFooter: Bool { row.kind == .footerUsage || row.kind == .footerHost }
    /// Tab and section rows hold the chevron slot even with nothing to open, so their glyphs
    /// and labels share one column. Runs never open; their indent already nests them.
    private var treeRow: Bool { row.kind == .tab || row.kind == .section }

    var body: some View {
        HStack(spacing: 5) {
            if row.chevron != "none", row.kind != .space, row.kind != .hidden, row.kind != .machine {
                chevron
            } else if treeRow {
                Color.clear.frame(width: Self.chevronWidth, height: 1)
            }
            if !row.glyph.isEmpty {
                Text(row.glyph).font(.system(size: 10)).foregroundStyle(tone).frame(width: Self.glyphWidth)
            }
            if row.kind == .goal {
                Text("goal").foregroundStyle(t.mute)
                goal
            } else {
                Text(row.title)
                    .font(.system(size: row.kind == .section ? 10.5 : 12.5,
                                  weight: row.kind == .space || row.kind == .title || row.kind == .machine ? .semibold
                                      : (row.kind == .tab && !row.dim ? .medium : .regular)))
                    .tracking(row.kind == .section ? 0.4 : 0)
                    .foregroundStyle(row.kind == .section || row.kind == .group || row.kind == .hidden || row.dim ? t.mute : t.ink)
                    .lineLimit(1).truncationMode(.tail)
                    .frame(minWidth: isFooter ? nil : Self.titleMinWidth, alignment: .leading)
                    // A footer's host name stays whole; its long summary is what gives way.
                    .layoutPriority(isFooter ? 2 : 0)
            }
            Spacer(minLength: 4)
            if !row.trailing.isEmpty, row.kind != .goal {
                trailingText.font(.system(size: 10.5)).monospacedDigit().foregroundStyle(row.link == nil ? t.mute : t.accent)
                    .lineLimit(1).truncationMode(.tail)
                    .frame(minWidth: Self.countWidth, alignment: .trailing)
                    .layoutPriority(1)
                    .onTapGesture { click(row.link == nil ? "body" : "link") }
            }
            if row.kind != .goal, !isFooter, row.kind != .title {
                Text(row.alert == "none" ? "" : "!").fontWeight(.bold)
                    .foregroundStyle(row.alert == "act" ? t.bad : t.warn)
                    .frame(width: Self.alertWidth)
            } else if row.alert != "none" {
                Text("!").fontWeight(.bold).foregroundStyle(row.alert == "act" ? t.bad : t.warn)
            }
            if showResume {
                Button("Resume") { resume() }.buttonStyle(.plain).foregroundStyle(t.accent).fixedSize()
            }
            if row.kind == .section {
                Text(focusMark ?? "").font(.system(size: 10)).foregroundStyle(t.mute)
                    .frame(width: 10).onTapGesture { if focusMark != nil { click("focus") } }
            }
            if row.kind == .tab {
                Text("⚲").foregroundStyle(pinned ? t.accent : t.mute).fixedSize().onTapGesture { click("pin") }
            }
            if row.kind == .space {
                Text("⚲").foregroundStyle(pinned ? t.accent : t.mute).fixedSize().onTapGesture { click("pin") }
                Text("+").foregroundStyle(t.mute).fixedSize().onTapGesture { click("plus") }
            }
            if row.kind == .space || row.kind == .hidden || row.kind == .machine, row.chevron != "none" {
                chevron
            }
        }
        .padding(.top, (row.kind == .space && row.id != firstSpaceId) || row.kind == .machine || row.id == "machines" ? 10 : 0)
        .frame(height: 23).padding(.leading, indent).padding(.horizontal, 4)
        .background(RoundedRectangle(cornerRadius: 4).fill(selected && row.kind == .tab ? t.sel : .clear))
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

    private var chevron: some View {
        Image(systemName: row.chevron == "open" ? "chevron.down" : "chevron.right").font(.system(size: 8, weight: .semibold)).foregroundStyle(t.mute)
            .frame(width: Self.chevronWidth).onTapGesture { click("chevron") }
    }

    /// Status color, as Ghostty: working green, blocked red, done peach, idle and unknown mute.
    private var tone: Color {
        switch row.tone {
        case "working": return t.ok
        case "blocked": return t.bad
        case "done": return t.warn
        default: return t.mute
        }
    }

    /// Leading inset: sections sit under the space name, rows under the section label.
    private var indent: CGFloat {
        switch row.kind {
        case .tab, .run: return CGFloat(row.depth) * 12 + 12
        case .section, .group: return CGFloat(row.depth) * 12
        default: return 0
        }
    }
}
