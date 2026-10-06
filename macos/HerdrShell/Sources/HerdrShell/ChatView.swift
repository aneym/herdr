import AppKit
import SwiftUI

// The rendered chat for one Claude pane (P19). It reads Transcript and ChatSender and
// decides nothing. T3 Code is the bar: one calm column, prose on the page, You on a soft
// tint, quiet tool rows, one hairline around the composer. No edge stripes, no box in a box.

/// Type scale and rhythm. Body is 14 pt at a 1.5 line height (21 pt).
private enum Metric {
    static let column: CGFloat = 760
    static let gutter: CGFloat = 28
    static let body: CGFloat = 14
    static let leading: CGFloat = 4.5
    static let small: CGFloat = 12.5
    static let caption: CGFloat = 11.5
    static let code: CGFloat = 12.5
    static let itemGap: CGFloat = 18
    static let blockGap: CGFloat = 12
    static let toolRow: CGFloat = 22
}

/// Colors for the chat, all derived from the shell's Theme tokens. Diff red/green are the
/// only hues Theme does not carry; they are tints here, never text on their own.
private struct Palette {
    let t: Tokens
    var dark: Bool { t.mode == .dark }
    var page: Color { Color(hex: t.terminalBg) }
    var ink: Color { t.ink }
    var mute: Color { t.mute }
    var faint: Color { t.mute.opacity(0.75) }
    var hair: Color { t.line }
    var you: Color { t.sel }
    var surface: Color { t.ink.opacity(dark ? 0.055 : 0.045) }
    var field: Color { dark ? Color.white.opacity(0.035) : Color.white }
    var link: Color { t.orch }
    var add: Color { t.ok }
    var del: Color { Color(hex: dark ? 0xF4746B : 0xB42318) }
    var addFill: Color { Color(hex: dark ? 0x2EA043 : 0x1A7F37).opacity(dark ? 0.18 : 0.10) }
    var delFill: Color { Color(hex: dark ? 0xF85149 : 0xCF222E).opacity(dark ? 0.18 : 0.09) }
    var warnFill: Color { t.warn.opacity(dark ? 0.14 : 0.10) }
}

/// The Ghostty font for code: the last non-empty `font-family` in the merged config.
enum ChatFont {
    static func family(config: String) -> String {
        let names = config.components(separatedBy: "\n").compactMap { line -> String? in
            guard TerminalTheme.key(of: line) == "font-family", let eq = line.firstIndex(of: "=") else { return nil }
            let v = line[line.index(after: eq)...].trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            return v.isEmpty ? nil : v
        }
        return names.last ?? "SF Mono"
    }
    static func mono(_ family: String, _ size: CGFloat, weight: NSFont.Weight = .regular) -> Font {
        if let f = NSFontManager.shared.font(withFamily: family, traits: [], weight: weight == .medium ? 6 : 5, size: size) { return Font(f as CTFont) }
        return .system(size: size, weight: weight == .medium ? .medium : .regular, design: .monospaced)
    }
}

// MARK: Markdown

private enum Block {
    case heading(Int, String)
    case paragraph(String)
    case list([(marker: String, text: String, depth: Int)])
    case code(lang: String, body: String)
    case table([[String]])
    case quote(String)
    case rule
}

/// Block-level markdown for assistant text: headings, paragraphs (soft breaks joined),
/// bullet and numbered lists, fenced code, pipe tables, quotes and rules. Inline syntax
/// (bold, italic, inline code, links) goes through AttributedString.
private func blocks(_ text: String) -> [Block] {
    var out: [Block] = [], para: [String] = []
    let lines = text.components(separatedBy: "\n")
    func flush() { if !para.isEmpty { out.append(.paragraph(para.joined(separator: " "))); para = [] } }
    func listItem(_ line: String) -> (String, String, Int)? {
        let indent = line.prefix(while: { $0 == " " }).count
        let t = line.trimmingCharacters(in: .whitespaces)
        if let f = t.first, "-*+".contains(f), t.dropFirst().hasPrefix(" ") { return ("•", String(t.dropFirst(2)), indent / 2) }
        let digits = t.prefix(while: \.isNumber)
        if !digits.isEmpty, t.dropFirst(digits.count).hasPrefix(". ") || t.dropFirst(digits.count).hasPrefix(") ") {
            return (digits + ".", String(t.dropFirst(digits.count + 2)), indent / 2)
        }
        return nil
    }
    var i = 0
    while i < lines.count {
        let line = lines[i], t = line.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("```") {
            flush()
            var body: [String] = []
            i += 1
            while i < lines.count, !lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("```") { body.append(lines[i]); i += 1 }
            out.append(.code(lang: String(t.dropFirst(3)).trimmingCharacters(in: .whitespaces), body: body.joined(separator: "\n")))
        } else if t.isEmpty {
            flush()
        } else if let level = Optional(t.prefix(while: { $0 == "#" }).count), (1...6).contains(level), t.dropFirst(level).hasPrefix(" ") {
            flush(); out.append(.heading(level, String(t.dropFirst(level + 1))))
        } else if t == "---" || t == "***" || t == "___" {
            flush(); out.append(.rule)
        } else if t.hasPrefix("|") {
            flush()
            var rows: [[String]] = []
            while i < lines.count, lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("|") {
                let row = lines[i].trimmingCharacters(in: .whitespaces)
                if !row.allSatisfy({ "|:- ".contains($0) }) {
                    rows.append(row.split(separator: "|", omittingEmptySubsequences: false).dropFirst().dropLast().map { $0.trimmingCharacters(in: .whitespaces) })
                }
                i += 1
            }
            out.append(.table(rows)); continue
        } else if t.hasPrefix(">") {
            flush()
            var q: [String] = []
            while i < lines.count, lines[i].trimmingCharacters(in: .whitespaces).hasPrefix(">") {
                q.append(lines[i].trimmingCharacters(in: .whitespaces).dropFirst().trimmingCharacters(in: .whitespaces)); i += 1
            }
            out.append(.quote(q.joined(separator: " "))); continue
        } else if listItem(line) != nil {
            flush()
            var items: [(marker: String, text: String, depth: Int)] = []
            while i < lines.count {
                if let (m, x, d) = listItem(lines[i]) { items.append((m, x, d)) }
                else if !items.isEmpty, lines[i].hasPrefix("  "), !lines[i].trimmingCharacters(in: .whitespaces).isEmpty { items[items.count - 1].text += " " + lines[i].trimmingCharacters(in: .whitespaces) }
                else { break }
                i += 1
            }
            out.append(.list(items)); continue
        } else {
            para.append(t)
        }
        i += 1
    }
    flush()
    return out
}

private struct Markdown: View {
    let text: String
    let p: Palette
    let codeFamily: String

    var body: some View {
        VStack(alignment: .leading, spacing: Metric.blockGap) {
            ForEach(Array(blocks(text).enumerated()), id: \.offset) { _, block in view(block) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func inline(_ s: String, size: CGFloat = Metric.body) -> Text {
        var a = (try? AttributedString(markdown: s, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(s)
        for run in a.runs {
            if run.inlinePresentationIntent?.contains(.code) == true {
                a[run.range].font = ChatFont.mono(codeFamily, size - 1)
                a[run.range].backgroundColor = p.surface
            }
            if run.link != nil { a[run.range].foregroundColor = p.link }
        }
        return Text(a)
    }

    @ViewBuilder private func view(_ block: Block) -> some View {
        switch block {
        case .heading(let level, let s):
            inline(s, size: level == 1 ? 19 : level == 2 ? 16.5 : 14.5)
                .font(.system(size: level == 1 ? 19 : level == 2 ? 16.5 : 14.5, weight: .semibold))
                .padding(.top, level <= 2 ? 6 : 2)
        case .paragraph(let s):
            inline(s).font(.system(size: Metric.body)).lineSpacing(Metric.leading)
        case .list(let items):
            VStack(alignment: .leading, spacing: 5) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(item.marker).font(.system(size: Metric.body)).foregroundStyle(p.mute)
                            .frame(minWidth: 12, alignment: item.marker == "•" ? .center : .trailing)
                        inline(item.text).font(.system(size: Metric.body)).lineSpacing(Metric.leading)
                    }
                    .padding(.leading, CGFloat(item.depth) * 18)
                }
            }
        case .code(let lang, let code):
            VStack(alignment: .leading, spacing: 6) {
                if !lang.isEmpty { Text(lang).font(.system(size: Metric.caption)).foregroundStyle(p.faint) }
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(code).font(ChatFont.mono(codeFamily, Metric.code)).lineSpacing(3).fixedSize()
                }
            }
            .padding(.horizontal, 14).padding(.vertical, 11)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(p.surface, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        case .table(let rows):
            Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 0) {
                ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                    GridRow {
                        ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                            inline(cell, size: 13.5).font(.system(size: 13.5, weight: index == 0 ? .semibold : .regular))
                                .foregroundStyle(index == 0 ? p.ink : p.ink.opacity(0.92))
                                .padding(.vertical, 7)
                        }
                    }
                    if index < rows.count - 1 {
                        Rectangle().fill(p.hair).frame(height: 1).gridCellUnsizedAxes(.horizontal)
                    }
                }
            }
        case .quote(let s):
            inline(s).font(.system(size: Metric.body)).italic().foregroundStyle(p.mute).lineSpacing(Metric.leading)
                .padding(.leading, 14)
        case .rule:
            Rectangle().fill(p.hair).frame(height: 1).padding(.vertical, 4)
        }
    }
}

// MARK: Tool rows

private struct ToolRow: View {
    let item: ChatItem
    let p: Palette
    let codeFamily: String
    @Binding var expanded: Bool
    @State private var hover = false

    private var icon: String {
        switch item.tool {
        case "Bash": return "terminal"
        case "Read": return "doc.text"
        case "Edit", "Write", "MultiEdit": return "pencil"
        case "Grep", "Glob": return "magnifyingglass"
        case "Agent", "Task": return "person.2"
        case "WebFetch", "WebSearch": return "globe"
        default: return "wrench.adjustable"
        }
    }
    private var isDiff: Bool { ["Edit", "Write", "MultiEdit"].contains(item.tool ?? "") }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button { expanded.toggle() } label: {
                HStack(spacing: 8) {
                    ZStack {
                        if item.status == "running" {
                            ProgressView().controlSize(.mini)
                        } else {
                            Image(systemName: icon).font(.system(size: 11, weight: .regular))
                                .foregroundStyle(item.status == "error" ? p.del : p.mute)
                        }
                    }
                    .frame(width: 16)
                    Text(item.tool ?? "Tool").font(.system(size: Metric.small, weight: .medium)).foregroundStyle(p.ink.opacity(0.78))
                    Text(item.text).font(.system(size: Metric.small)).foregroundStyle(p.mute).lineLimit(1).truncationMode(.middle)
                    if item.status == "error" { Text("failed").font(.system(size: Metric.small)).foregroundStyle(p.del) }
                    Spacer(minLength: 8)
                    Image(systemName: "chevron.right").font(.system(size: 9, weight: .semibold)).foregroundStyle(p.faint)
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                        .opacity(hover || expanded ? 1 : 0)
                }
                .frame(height: Metric.toolRow)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hover = $0 }
            if expanded { detail.padding(.leading, 24) }
        }
    }

    @ViewBuilder private var detail: some View {
        let mono = ChatFont.mono(codeFamily, 12)
        VStack(alignment: .leading, spacing: 0) {
            if isDiff {
                ForEach(Array(diffLines.enumerated()), id: \.offset) { _, line in
                    HStack(alignment: .firstTextBaseline, spacing: 0) {
                        Text(line.sign).foregroundStyle(line.sign == "+" ? p.add : line.sign == "-" ? p.del : p.faint).frame(width: 18, alignment: .center)
                        Text(line.text.isEmpty ? " " : line.text).foregroundStyle(p.ink.opacity(0.9))
                    }
                    .font(mono)
                    .padding(.vertical, 2.5).padding(.trailing, 12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Rectangle().fill(line.sign == "+" ? p.addFill : line.sign == "-" ? p.delFill : Color.clear))
                }
                if let r = item.result, item.status == "error" {
                    Text(r).font(mono).foregroundStyle(p.del).textSelection(.enabled).padding(12)
                }
            } else {
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: 10) {
                        Text(item.input ?? "").font(mono).foregroundStyle(p.ink.opacity(0.85)).textSelection(.enabled)
                        if let r = item.result, !r.isEmpty {
                            Rectangle().fill(p.hair).frame(height: 1)
                            Text(r).font(mono).foregroundStyle(item.status == "error" ? p.del : p.mute).textSelection(.enabled)
                        }
                    }
                    .padding(12).frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 260)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(p.surface, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }

    private var diffLines: [(sign: String, text: String)] {
        (item.input ?? "").components(separatedBy: "\n")
            .filter { !$0.hasPrefix("--- ") && !$0.hasPrefix("+++ ") }
            .map { l in
                guard let f = l.first, f == "+" || f == "-" else { return (" ", l) }
                return (String(f), String(l.dropFirst()))
            }
    }
}

private struct ToolGroup: View {
    let items: [ChatItem]
    let p: Palette
    let codeFamily: String
    @Binding var open: Bool
    let expanded: (String) -> Binding<Bool>
    let ui: ChatUI
    @State private var hover = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if items.count > 1 {
                Button { open.toggle() } label: {
                    HStack(spacing: 8) {
                        ZStack {
                            if items.contains(where: { $0.status == "running" }) && !open {
                                ProgressView().controlSize(.mini)
                            } else {
                                Image(systemName: "chevron.right").font(.system(size: 9, weight: .semibold)).foregroundStyle(p.faint)
                                    .rotationEffect(.degrees(open ? 90 : 0))
                            }
                        }
                        .frame(width: 16)
                        Text("\(items.count) tool calls").font(.system(size: Metric.small, weight: .medium)).foregroundStyle(p.ink.opacity(0.78))
                        Text("·  " + names).font(.system(size: Metric.small)).foregroundStyle(p.mute).lineLimit(1)
                        if items.contains(where: { $0.status == "error" }) {
                            Text("·  \(items.filter { $0.status == "error" }.count) failed").font(.system(size: Metric.small)).foregroundStyle(p.del)
                        }
                        Spacer()
                    }
                    .frame(height: Metric.toolRow).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            if items.count == 1 || open {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(items) { item in
                        ToolRow(item: item, p: p, codeFamily: codeFamily, expanded: expanded(item.id))
                            .onAppear { ui.rendered.insert(item.id) }.onDisappear { ui.rendered.remove(item.id) }
                    }
                }
                .padding(.leading, items.count > 1 ? 24 : 0)
            }
        }
    }

    private var names: String {
        var seen: [String] = []
        for i in items { if let t = i.tool, !seen.contains(t) { seen.append(t) } }
        return seen.joined(separator: ", ")
    }
}

// MARK: Chat

/// View state that outlives a lazy row: Focus or Full, and which tool runs and rows the
/// person opened. Focus is the default (Alex, 2026-10-01: "default to focus mode, just
/// like claude code"); the choice persists in UserDefaults unless a flag pins it.
final class ChatUI: ObservableObject {
    enum Mode: String { case focus, full }
    static let modeKey = "HerdrShell.chatMode"
    @Published var mode: Mode { didSet { if persist { UserDefaults.standard.set(mode.rawValue, forKey: Self.modeKey) } } }
    @Published var groupOpen: [String: Bool] = [:]
    @Published var toolOpen: [String: Bool] = [:]
    /// Tool rows currently on screen (for the check's state dump).
    var rendered = Set<String>()
    private let persist: Bool

    init(pinned: Mode? = nil) {
        persist = pinned == nil
        mode = pinned ?? Mode(rawValue: UserDefaults.standard.string(forKey: Self.modeKey) ?? "") ?? .focus
    }

    /// Runs of several calls open only in Full, or once clicked.
    func isOpen(_ group: [ChatItem]) -> Bool { groupOpen[group[0].id] ?? (mode == .full) }
    /// What a click on a run's line does.
    func toggle(_ group: [ChatItem]) { groupOpen[group[0].id] = !isOpen(group) }
    /// In Full, small diffs open so a change reads without a click; Focus keeps every row shut.
    func isOpen(tool item: ChatItem) -> Bool {
        if let v = toolOpen[item.id] { return v }
        guard mode == .full, ["Edit", "Write", "MultiEdit"].contains(item.tool ?? "") else { return false }
        return (item.input ?? "").components(separatedBy: "\n").count <= 26
    }
    func toggle(tool item: ChatItem) { toolOpen[item.id] = !isOpen(tool: item) }
    /// A switch of mode resets what was opened by hand, so the switch shows its whole effect.
    func set(_ m: Mode) { groupOpen = [:]; toolOpen = [:]; mode = m }

    /// The live line while the agent works: the newest running tool, else thinking.
    static func activity(_ items: [ChatItem]) -> String {
        guard let t = items.last(where: { $0.kind == "tool" && $0.status == "running" }) else { return "Thinking…" }
        let base = (t.text as NSString).lastPathComponent
        switch t.tool ?? "" {
        case "Read": return "Reading \(base)"
        case "Edit", "Write", "MultiEdit": return "Editing \(base)"
        case "Grep", "Glob": return "Searching"
        case "WebFetch", "WebSearch": return "Searching the web"
        case "Agent", "Task": return t.text.isEmpty ? "Delegating" : "Delegating: \(t.text)"
        case "Bash": return t.text.isEmpty ? "Running a command" : t.text
        default: return t.text.isEmpty ? (t.tool ?? "Working") : "\(t.tool ?? ""): \(t.text)"
        }
    }
}

private struct ChatGroup: Identifiable {
    var items: [ChatItem]
    var id: String { items[0].id }
}

struct ChatView: View {
    @ObservedObject var transcript: Transcript
    @ObservedObject var theme: ThemeStore
    @ObservedObject var sender: ChatSender
    @ObservedObject var ui: ChatUI
    let codeFamily: String
    @State private var text = ""
    @State private var composerHeight: CGFloat = 21
    @State private var stick = true
    @State private var wheel: Any?

    private var groups: [ChatGroup] {
        var groups: [ChatGroup] = []
        for item in transcript.items {
            if item.kind == "tool", groups.last?.items.last?.kind == "tool" { groups[groups.count - 1].items.append(item) }
            else { groups.append(ChatGroup(items: [item])) }
        }
        return groups
    }

    var body: some View {
        let p = Palette(t: theme.tokens)
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: Metric.itemGap) {
                        if transcript.earlier {
                            Button { stick = false; transcript.loadEarlier() } label: {
                                Text("Load earlier messages").font(.system(size: Metric.small)).foregroundStyle(p.mute)
                            }
                            .buttonStyle(.plain).frame(maxWidth: .infinity)
                        }
                        ForEach(groups) { group in row(group, p).id(group.id) }
                        if !sender.pending.isEmpty { pendingBubble(p) }
                        // Only the user's own scrolling (the wheel monitor below) unsticks; layout
                        // changes that push this marker off screen must not.
                        Color.clear.frame(height: 1).id("latest").onAppear { stick = true }
                    }
                    .padding(.horizontal, Metric.gutter).padding(.top, 12).padding(.bottom, 12)
                    .frame(maxWidth: Metric.column).frame(maxWidth: .infinity)
                }
                .scrollIndicators(.never)
                .mask(VStack(spacing: 0) { LinearGradient(colors: [.clear, .black], startPoint: .top, endPoint: .bottom).frame(height: 14); Color.black })
                .overlay { if transcript.items.isEmpty { empty(p) } }
                .onChange(of: transcript.items) { _ in
                    sender.acknowledge(transcript.items)
                    if stick { toBottom(proxy) }
                }
                .onChange(of: sender.pending) { _ in if stick { toBottom(proxy) } }
                .onAppear {
                    toBottom(proxy)
                    wheel = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { event in
                        if event.scrollingDeltaY > 0 { stick = false }
                        return event
                    }
                }
                .onDisappear { if let wheel { NSEvent.removeMonitor(wheel) }; wheel = nil }
                .overlay(alignment: .bottom) {
                    if !stick && !transcript.items.isEmpty {
                        Button { stick = true; withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo("latest", anchor: .bottom) } } label: {
                            HStack(spacing: 6) {
                                Image(systemName: "arrow.down").font(.system(size: 10, weight: .semibold))
                                Text("Jump to latest").font(.system(size: Metric.small, weight: .medium))
                            }
                            .foregroundStyle(p.ink.opacity(0.85))
                            .padding(.horizontal, 12).frame(height: 28)
                            .background(Capsule().fill(p.page))
                            .overlay(Capsule().stroke(p.hair, lineWidth: 1))
                            .shadow(color: .black.opacity(p.dark ? 0.4 : 0.08), radius: 8, y: 2)
                        }
                        .buttonStyle(.plain).padding(.bottom, 10)
                    }
                }
            }
            footer(p)
                .padding(.horizontal, Metric.gutter).padding(.bottom, 18)
                .frame(maxWidth: Metric.column).frame(maxWidth: .infinity)
        }
        .foregroundStyle(p.ink)
        .background(p.page)
        .preferredColorScheme(theme.effective == .dark ? .dark : .light)
    }

    /// Scroll to the end now and once more after the lazy stack has measured new rows.
    private func toBottom(_ proxy: ScrollViewProxy) {
        proxy.scrollTo("latest", anchor: .bottom)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { proxy.scrollTo("latest", anchor: .bottom) }
    }

    // One transcript item (or a run of tool calls).
    @ViewBuilder private func row(_ group: ChatGroup, _ p: Palette) -> some View {
        let item = group.items[0]
        switch item.kind {
        case "tool":
            ToolGroup(items: group.items, p: p, codeFamily: codeFamily,
                      open: Binding(get: { ui.isOpen(group.items) }, set: { _ in ui.toggle(group.items) }),
                      expanded: { id in
                          Binding(get: { group.items.first { $0.id == id }.map(ui.isOpen(tool:)) ?? false },
                                  set: { _ in if let item = group.items.first(where: { $0.id == id }) { ui.toggle(tool: item) } })
                      },
                      ui: ui)
        case "user":
            youBubble(item.text, p)
            if item.queued {
                HStack(spacing: 4) {
                    Spacer()
                    Image(systemName: "clock").font(.system(size: 10))
                    Text("Queued until the current step ends").font(.system(size: Metric.caption))
                }
                .foregroundStyle(p.mute).padding(.top, -10)
            }
        case "assistant":
            Markdown(text: item.text, p: p, codeFamily: codeFamily)
        case "duration":
            HStack(spacing: 12) {
                Rectangle().fill(p.hair).frame(height: 1)
                Text("Worked for \(item.text)").font(.system(size: Metric.caption)).foregroundStyle(p.mute).fixedSize()
                Rectangle().fill(p.hair).frame(height: 1)
            }
            .padding(.vertical, 6)
        default:
            note(item.text, p)
        }
    }

    private func youBubble(_ s: String, _ p: Palette) -> some View {
        HStack {
            Spacer(minLength: 96)
            Text(s).font(.system(size: Metric.body)).lineSpacing(Metric.leading).textSelection(.enabled)
                .padding(.horizontal, 14).padding(.vertical, 9)
                .background(p.you, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        }
    }

    private func pendingBubble(_ p: Palette) -> some View {
        VStack(alignment: .trailing, spacing: 6) {
            youBubble(sender.pending, p).opacity(0.55)
            Text(sender.warning ? "Not sent" : sender.status.hasPrefix("held") ? "Held: sends when the terminal stops asking" : "Sending")
                .font(.system(size: Metric.caption)).foregroundStyle(p.mute)
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
    }

    /// A lane bulletin: "from <pane>: <text>", pane in medium weight.
    private func note(_ s: String, _ p: Palette) -> some View {
        var pane = "", body = s
        if s.hasPrefix("from "), let colon = s.range(of: ": ") {
            pane = String(s[s.index(s.startIndex, offsetBy: 5)..<colon.lowerBound]); body = String(s[colon.upperBound...])
        }
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "arrow.turn.down.right").font(.system(size: 10)).frame(width: 16)
            (Text(pane.isEmpty ? "" : pane + "  ").fontWeight(.medium).foregroundColor(p.ink.opacity(0.78)) + Text(body))
                .font(.system(size: Metric.small)).lineLimit(2)
        }
        .foregroundStyle(p.mute)
    }

    @ViewBuilder private func empty(_ p: Palette) -> some View {
        VStack(spacing: 8) {
            if transcript.waiting {
                ProgressView().controlSize(.small)
                Text("Waiting for the session").font(.system(size: Metric.body, weight: .medium))
                Text("The chat appears once Claude writes its transcript.").font(.system(size: Metric.small)).foregroundStyle(p.mute)
            } else if transcript.state == "asleep" && !sender.readOnly {
                Text("No Claude session in this pane").font(.system(size: Metric.body, weight: .medium))
                Text("Start Claude in the terminal and the chat follows it.").font(.system(size: Metric.small)).foregroundStyle(p.mute)
            } else {
                Text("Nothing here yet").font(.system(size: Metric.body, weight: .medium))
                Text("Messages to \(transcript.name) and its replies appear here.").font(.system(size: Metric.small)).foregroundStyle(p.mute)
            }
        }
        .multilineTextAlignment(.center)
    }

    // MARK: Footer: status line, draft warning, composer

    @ViewBuilder private func footer(_ p: Palette) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            status(p)
            if sender.warning {
                HStack(spacing: 10) {
                    Image(systemName: "exclamationmark.circle").font(.system(size: 13)).foregroundStyle(p.t.warn)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(sender.status).font(.system(size: 13, weight: .medium))
                        Text(sender.status.contains("prompt") ? "The prompt isn't in view, so this may append to a draft." : "Sending now would join it to your message.").font(.system(size: Metric.small)).foregroundStyle(p.mute)
                    }
                    Spacer()
                    Button("Cancel") { text = sender.cancel() }.buttonStyle(.plain).font(.system(size: Metric.small, weight: .medium)).foregroundStyle(p.mute)
                    Button { sender.send(sender.pending, anyway: true, known: transcript.items.map(\.id)) } label: {
                        Text("Send anyway").font(.system(size: Metric.small, weight: .medium)).foregroundStyle(p.page)
                            .padding(.horizontal, 10).frame(height: 24).background(p.ink, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 12).padding(.vertical, 10)
                .background(p.warnFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
            composer(p)
        }
    }

    @ViewBuilder private func status(_ p: Palette) -> some View {
        let message = sender.warning || sender.status.isEmpty || sender.status == "sending" || sender.status.hasPrefix("held") ? "" : sender.status
        HStack(spacing: 8) {
            switch transcript.state {
            case "working":
                TimelineView(.periodic(from: .now, by: 0.5)) { context in
                    let phase = Int(context.date.timeIntervalSinceReferenceDate * 2) % 2 == 0
                    HStack(spacing: 8) {
                        Circle().fill(p.t.ok).frame(width: 7, height: 7).opacity(phase ? 1 : 0.35)
                            .animation(.easeInOut(duration: 0.45), value: phase)
                        Text(ChatUI.activity(transcript.items)).font(.system(size: Metric.small, weight: .medium)).lineLimit(1)
                        Text(Self.elapsed(context.date.timeIntervalSince(transcript.workingSince))).font(.system(size: Metric.small).monospacedDigit()).foregroundStyle(p.mute)
                    }
                }
            case "blocked":
                Circle().fill(p.t.warn).frame(width: 7, height: 7)
                Text("Needs you").font(.system(size: Metric.small, weight: .semibold)).foregroundStyle(p.t.warn)
                Text("The terminal is asking for permission. Switch to Terminal to answer.").font(.system(size: Metric.small)).foregroundStyle(p.mute).lineLimit(1)
            case "asleep":
                Circle().stroke(p.mute, lineWidth: 1.2).frame(width: 7, height: 7)
                Text("Asleep").font(.system(size: Metric.small, weight: .medium)).foregroundStyle(p.mute)
                Text("No Claude session in this pane").font(.system(size: Metric.small)).foregroundStyle(p.mute)
            default:
                EmptyView()
            }
            Spacer(minLength: 0)
            if !message.isEmpty { Text(message).font(.system(size: Metric.small)).foregroundStyle(p.mute).lineLimit(1) }
        }
        .frame(height: 18)
        .padding(.leading, 4)
    }

    static func elapsed(_ s: TimeInterval) -> String {
        let n = max(0, Int(s))
        return n < 60 ? "\(n)s" : n < 3600 ? "\(n / 60)m \(n % 60)s" : "\(n / 3600)h \((n % 3600) / 60)m"
    }

    private func composer(_ p: Palette) -> some View {
        let empty = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        return VStack(alignment: .leading, spacing: 8) {
            ComposerField(text: $text, height: $composerHeight, ink: NSColor(hex: p.t.chrome.ink), submit: submit)
                .frame(height: min(max(composerHeight, 21), 200))
                .overlay(alignment: .topLeading) {
                    if text.isEmpty {
                        Text("Message \(transcript.name)").font(.system(size: Metric.body)).foregroundStyle(p.faint)
                            .padding(.top, 1).allowsHitTesting(false)
                    }
                }
            HStack(spacing: 8) {
                Text("Enter to send  ·  Shift+Enter for a new line").font(.system(size: Metric.caption)).foregroundStyle(p.faint)
                Spacer()
                Button { submit(false) } label: {
                    Image(systemName: "arrow.up").font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(empty ? p.mute : p.page)
                        .frame(width: 26, height: 26)
                        .background(Circle().fill(empty ? p.hair : p.ink))
                }
                .buttonStyle(.plain).disabled(empty).help("Send (Enter)")
            }
        }
        .padding(.horizontal, 14).padding(.top, 12).padding(.bottom, 10)
        .background(p.field, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(p.hair, lineWidth: 1))
        .shadow(color: .black.opacity(p.dark ? 0 : 0.035), radius: 6, y: 2)
    }

    private func submit(_ force: Bool) {
        let message = text.isEmpty && force ? sender.pending : text
        sender.send(message, anyway: force, known: transcript.items.map(\.id))
        if !sender.readOnly && !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && message.count <= 20_000 { text = "" }
    }
}

/// The composer's text view. Enter sends, Shift+Enter is a newline, Cmd+Enter sends past a
/// draft warning. Reports its content height so the field grows with the text.
private struct ComposerField: NSViewRepresentable {
    @Binding var text: String
    @Binding var height: CGFloat
    let ink: NSColor
    var submit: (Bool) -> Void

    final class Editor: NSTextView {
        var submit: ((Bool) -> Void)?
        override func keyDown(with event: NSEvent) {
            if event.keyCode == 36 || event.keyCode == 76, !event.modifierFlags.contains(.shift), !hasMarkedText() {
                submit?(event.modifierFlags.contains(.command)); return
            }
            super.keyDown(with: event)
        }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { DispatchQueue.main.async { window.makeFirstResponder(self) } }
        }
    }
    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ComposerField
        init(_ parent: ComposerField) { self.parent = parent }
        func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            parent.text = view.string
            measure(view)
        }
        func measure(_ view: NSTextView) {
            guard let lm = view.layoutManager, let tc = view.textContainer else { return }
            lm.ensureLayout(for: tc)
            let h = ceil(lm.usedRect(for: tc).height)
            if abs(h - parent.height) > 0.5 { DispatchQueue.main.async { self.parent.height = h } }
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView(), editor = Editor()
        editor.isRichText = false
        editor.allowsUndo = true
        editor.font = .systemFont(ofSize: Metric.body)
        let para = NSMutableParagraphStyle(); para.lineSpacing = Metric.leading - 1
        editor.defaultParagraphStyle = para
        editor.typingAttributes[.paragraphStyle] = para
        editor.drawsBackground = false
        editor.textContainerInset = .zero
        editor.textContainer?.lineFragmentPadding = 0
        editor.isVerticallyResizable = true; editor.autoresizingMask = [.width]; editor.textContainer?.widthTracksTextView = true
        editor.isAutomaticQuoteSubstitutionEnabled = false; editor.isAutomaticDashSubstitutionEnabled = false
        editor.delegate = context.coordinator; editor.submit = submit
        editor.setAccessibilityLabel("Message")
        scroll.documentView = editor
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        return scroll
    }
    func updateNSView(_ view: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let editor = view.documentView as? Editor else { return }
        if editor.string != text { editor.string = text; context.coordinator.measure(editor) }
        editor.textColor = ink
        editor.insertionPointColor = ink
        editor.submit = submit
    }
}

// MARK: Demo window

final class ChatDemoDelegate: NSObject, NSApplicationDelegate {
    /// A check or a read-only render: no window on screen, no focus, no Dock icon.
    static var offscreen: Bool { args["dump-chat"] != nil || args["control"] != nil || flags.contains("--read-only") }
    var window: NSWindow?
    var transcript: Transcript?
    var sender: ChatSender?
    var ui: ChatUI?
    var hook: ChatHook?
    func applicationDidFinishLaunching(_ note: Notification) {
        let theme = ThemeStore(override: appearanceOverride, glass: GlassTokens(), terminal: terminalTheme)
        theme.startFollowingSystem()
        let pane = args["pane"]
        let transcript = Transcript(pane: pane, file: args["transcript"], dump: args["dump-chat"],
                                    state: pane == nil ? args["chat-state"] : nil, name: args["agent-name"])
        let sender = ChatSender(pane: pane ?? "", readOnly: flags.contains("--read-only"))
        self.transcript = transcript; self.sender = sender
        let ui = ChatUI(pinned: args["chat-mode"].flatMap(ChatUI.Mode.init(rawValue:)))
        self.ui = ui
        let view = ChatView(transcript: transcript, theme: theme, sender: sender, ui: ui, codeFamily: ChatFont.family(config: ghosttyConfigText))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 900, height: 900), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        window.title = pane.map { "Chat · \($0)" } ?? "Chat"
        window.appearance = theme.nsAppearance
        window.contentView = NSHostingView(rootView: view)
        window.center()
        // Script-driven and read-only windows are never ordered on screen (main.swift also
        // makes them background-only): captures draw in-process, keys go to the window
        // object. Only an interactive `--demo chat --pane` comes to the front.
        if !ChatDemoDelegate.offscreen {
            window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        }
        self.window = window
        if let dump = args["dump-chat"] {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { ChatHook.capture(window, to: dump + ".png") }
            let timer = Timer(timeInterval: 0.25, repeats: true) { _ in
                let view = ChatHook.view(transcript, ui).merging(["window_on_screen": window.isVisible, "app_active": NSApp.isActive]) { a, _ in a }
                if let data = try? JSONSerialization.data(withJSONObject: view, options: [.sortedKeys]) {
                    try? data.write(to: URL(fileURLWithPath: dump + ".view.json"), options: .atomic)
                }
            }
            RunLoop.main.add(timer, forMode: .common)
        }
        if let fifo = args["control"] {
            hook = ChatHook(path: fifo, window: window, transcript: transcript, sender: sender, ui: ui)
            hook?.start()
        }
        let menu = NSMenu(), appItem = NSMenuItem(), appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu; menu.addItem(appItem)
        let editItem = NSMenuItem(), editMenu = NSMenu(title: "Edit")
        for (title, sel, key) in [("Undo", Selector(("undo:")), "z"), ("Cut", #selector(NSText.cut(_:)), "x"), ("Copy", #selector(NSText.copy(_:)), "c"),
                                  ("Paste", #selector(NSText.paste(_:)), "v"), ("Select All", #selector(NSText.selectAll(_:)), "a")] {
            editMenu.addItem(withTitle: title, action: sel, keyEquivalent: key)
        }
        editItem.submenu = editMenu; menu.addItem(editItem)
        NSApp.mainMenu = menu
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
