import SwiftUI

final class PaneChat {
    let transcript: Transcript
    let sender: ChatSender
    let ui: ChatUI
    let view: NSView
    init(transcript: Transcript, sender: ChatSender, ui: ChatUI, view: NSView) {
        self.transcript = transcript
        self.sender = sender
        self.ui = ui
        self.view = view
    }
}

/// What one pane's 36 pt cap shows. The mode is per pane.
struct PaneCapState: Equatable {
    var paneId: String
    var name: String
    var agent: Bool
    var focused: Bool
    var chat: Bool
    var density: String
    var glyph: ShellState
    /// The tab's pin, a server fact, shown on the top-right pane's cap only (nil elsewhere).
    var pinned: Bool? = nil
}

struct PaneCapBar: View {
    var state: PaneCapState
    var tokens: Tokens
    var onTerminal: () -> Void
    var onChat: () -> Void
    var onFocus: () -> Void
    var onFull: () -> Void
    var onPin: () -> Void = {}
    var onRestart: () -> Void = {}
    var onGrab: (NSPoint) -> Void = { _ in }

    var body: some View {
        let bg = state.focused ? Color(hex: tokens.terminalBg) : tokens.cap
        HStack(spacing: 8) {
            HStack(spacing: 8) {
                StateGlyph(state: state.glyph, tokens: tokens)
                Text(state.name.isEmpty ? "Brief" : state.name)
                    .font(.system(size: 12.5, weight: state.focused ? .medium : .regular))
                    .foregroundStyle(state.focused ? tokens.ink : tokens.mute)
                    .lineLimit(1)
                    .help("\(state.paneId) · \(state.name)")
                Spacer(minLength: 8)
            }.overlay { PaneCapGrab(onPress: onGrab) }
            if state.agent {
                if state.chat {
                    density("Focus", on: state.density != "full", action: onFocus)
                    density("Full", on: state.density == "full", action: onFull)
                    Rectangle().fill(tokens.line).frame(width: 1, height: 14)
                }
                segment
            }
            if let pinned = state.pinned { pin(pinned) }
            Menu {
                Button("Restart agent", systemImage: "arrow.clockwise", action: onRestart)
                    .disabled(!PaneRestart.enabled(hasAgent: state.agent))
            } label: {
                Image(systemName: "ellipsis")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(tokens.mute)
                    .frame(width: Self.pinWidth, height: 20)
                    .opacity(state.focused ? 1 : 0.55)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("Pane actions")
        }
        .padding(.horizontal, PaneCapBar.trailing)
        .frame(height: ShellSpace.paneCapHeight)
        .background(bg)
        .opacity(state.focused ? 1 : 0.92)
    }

    private func density(_ title: String, on: Bool, action: @escaping () -> Void) -> some View {
        Text(title)
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(on ? tokens.ink : tokens.mute)
            .padding(.horizontal, 6)
            .frame(height: 20)
            .background(RoundedRectangle(cornerRadius: 6).fill(on ? tokens.tint : Color.clear))
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }

    /// Same quiet language as the segment: tint behind it while the tab is pinned.
    private func pin(_ pinned: Bool) -> some View {
        Image(systemName: pinned ? "pin.slash" : "pin")
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(pinned ? tokens.ink : tokens.mute)
            .frame(width: PaneCapBar.pinWidth, height: 20)
            .background(RoundedRectangle(cornerRadius: 6).fill(pinned ? tokens.tint : Color.clear))
            .opacity(state.focused ? 1 : 0.55)
            .contentShape(Rectangle())
            .onTapGesture(perform: onPin)
            .help(pinned ? "Unpin this chat" : "Pin this chat to the end of Pinned")
    }

    /// The pin's width and the bar's trailing padding: the test hook clicks its centre.
    static let pinWidth: CGFloat = 24
    static let trailing: CGFloat = 10

    private var segment: some View {
        HStack(spacing: 0) {
            seg("Terminal", on: !state.chat, action: onTerminal)
            seg("Chat", on: state.chat, action: onChat)
        }
        .padding(2)
        .background(RoundedRectangle(cornerRadius: 7).fill(tokens.tint))
        .opacity(state.focused ? 1 : 0.55)
    }

    private func seg(_ title: String, on: Bool, action: @escaping () -> Void) -> some View {
        Text(title)
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(on ? tokens.ink : tokens.mute)
            .padding(.horizontal, 8)
            .frame(height: 20)
            .background(
                RoundedRectangle(cornerRadius: 5)
                    .fill(on ? (tokens.mode == .dark ? tokens.sel : Color(hex: tokens.terminalBg)) : Color.clear)
                    .overlay(RoundedRectangle(cornerRadius: 5).stroke(on && tokens.mode == .light ? tokens.line : Color.clear, lineWidth: 0.5))
            )
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }
}

/// Only the title/empty region owns this native handle. SwiftUI controls remain siblings.
private struct PaneCapGrab: NSViewRepresentable {
    var onPress: (NSPoint) -> Void
    func makeNSView(context: Context) -> PaneCapGrabView { PaneCapGrabView() }
    func updateNSView(_ view: PaneCapGrabView, context: Context) { view.onPress = onPress }
}
private final class PaneCapGrabView: NSView {
    var onPress: (NSPoint) -> Void = { _ in }
    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .openHand) }
    override func mouseDown(with event: NSEvent) { onPress(event.locationInWindow) }
}
