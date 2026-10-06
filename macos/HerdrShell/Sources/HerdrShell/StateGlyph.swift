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
            RoundedRectangle(cornerRadius: 1.5 * scale).fill(tokens.bad).frame(width: 7.5 * scale, height: 7.5 * scale)
        case .idle:
            Circle().stroke(tokens.mute, lineWidth: 1.5 * scale).frame(width: s, height: s)
        case .asleep:
            RoundedRectangle(cornerRadius: 0.5).fill(tokens.faint).frame(width: 7 * scale, height: 1.5 * scale)
        case .done:
            Image(systemName: "checkmark").font(.system(size: 10 * scale, weight: .semibold)).foregroundStyle(tokens.faint)
        }
    }
}
