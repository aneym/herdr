import SwiftUI

/// One shape per state. Color is only the token; the shape still reads without it.
enum ShellState: String {
    case working, needs, blocked, idle, asleep, done

    static func from(status: String, failed: Bool, hasAgent: Bool) -> ShellState {
        if failed { return .blocked }
        switch status {
        case "working": return .working
        case "blocked": return .needs
        case "idle": return hasAgent ? .idle : .asleep
        case "done": return .done
        default: return hasAgent ? .idle : .asleep
        }
    }

    /// The herdr status the sidebar keys its glyph and tone on: needs and a failed run both read as
    /// blocked, idle and asleep as idle.
    var sidebarStatus: String {
        switch self {
        case .working: return "working"
        case .needs, .blocked: return "blocked"
        case .done: return "done"
        case .idle, .asleep: return "idle"
        }
    }
}

struct StateGlyph: View {
    var state: ShellState
    var tokens: Tokens
    var scale: CGFloat = 1

    var body: some View {
        let s = 8 * scale
        switch state {
        case .working:
            Circle().fill(tokens.ok).frame(width: s, height: s)
        case .needs:
            Rectangle().fill(tokens.warn).frame(width: 7 * scale, height: 7 * scale).rotationEffect(.degrees(45))
        case .blocked:
            RoundedRectangle(cornerRadius: ShellRadius.curve1_5 * scale).fill(tokens.bad).frame(width: 7.5 * scale, height: 7.5 * scale)
        case .idle:
            Circle().stroke(tokens.mute, lineWidth: 1.5 * scale).frame(width: s, height: s)
        case .asleep:
            RoundedRectangle(cornerRadius: ShellRadius.curve0_5).fill(tokens.faint).frame(width: 7 * scale, height: 1.5 * scale)
        case .done:
            Image(systemName: "checkmark").font(.system(size: ShellType.glyph * scale, weight: .semibold)).foregroundStyle(tokens.faint)
        }
    }
}
