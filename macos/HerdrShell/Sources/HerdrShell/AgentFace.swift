import AppKit
import SwiftUI

/// An AGENTS row's face in place of its state glyph: the agent's picture, else its initial on a tint
/// (Rails PersonFace). The state stays a small dot set into the lower right of the circle
/// (SpacesRow.faceDot); an idle agent shows none, and nothing here is a filled count or a red badge.
/// Sizes, tints and opacities are shell/tokens.json `face`, shared with the Windows client.
struct AgentFace: View {
    let face: SpacesRow.Face
    /// The dot's chrome token (ok, accent, warn), or nil for none.
    let dot: String?
    /// The row's tone (SpacesTree.mark), named on hover.
    let tone: String
    /// An open request id; it owns the dot and the hover.
    let request: String?
    let t: Tokens
    @ObservedObject private var pictures = FacePictures.shared

    var body: some View {
        let size = ShellFace.size, dot = ShellFace.dot, hole = ShellFace.dot + 2 * ShellFace.dotGap
        circle
            .frame(width: size, height: size)
            .mask {
                // Cut a gap around the dot so it reads on any sidebar, glass or selection plate.
                ZStack {
                    Circle()
                    if dotColor != nil {
                        let shift = (size - dot) / 2 + ShellFace.dotOffset / 2
                        Circle().frame(width: hole, height: hole).offset(x: shift, y: shift)
                            .blendMode(.destinationOut)
                    }
                }.compositingGroup()
            }
            .overlay(alignment: .bottomTrailing) {
                if let dotColor {
                    Circle().fill(dotColor).frame(width: dot, height: dot)
                        .offset(x: ShellFace.dotOffset, y: ShellFace.dotOffset)
                }
            }
            .task(id: face.avatar) { if let url = face.avatar { pictures.load(url) } }
            .help(hover)
    }

    @ViewBuilder private var circle: some View {
        if let url = face.avatar, let image = pictures.images[url] {
            // A picture brings its own background; a hairline ring gives it an edge in either theme.
            Image(nsImage: image).resizable().interpolation(.high).aspectRatio(contentMode: .fill)
                .overlay(Circle().strokeBorder(t.line, lineWidth: ShellFace.ring))
        } else {
            let tint = Color(hex: ShellFace.tints[face.tint % ShellFace.tints.count])
            ZStack {
                Circle().fill(tint.opacity(t.mode == .dark ? ShellFace.tintOpacityDark : ShellFace.tintOpacityLight))
                Text(face.initial).font(.system(size: ShellFace.initial, weight: .semibold)).foregroundStyle(t.ink)
            }
        }
    }

    private var hover: String {
        if let request { return "Needs you, request \(request)" }
        switch tone {
        case "working": return "Working"
        case "blocked": return "Needs you"
        case "done": return "Done"
        default: return ""
        }
    }

    /// Green while it works, blue when it needs you (the one quiet "for you" dot), peach once done.
    private var dotColor: Color? {
        switch dot {
        case "ok": return t.ok
        case "accent": return t.accent
        case "warn": return t.warn
        default: return nil
        }
    }
}

/// Agent pictures by URL, fetched once per run. A failed fetch leaves the initial in place.
final class FacePictures: ObservableObject {
    static let shared = FacePictures()
    @Published private(set) var images: [String: NSImage] = [:]
    private var asked = Set<String>()
    private let session: URLSession = {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 10
        return URLSession(configuration: config)
    }()

    func load(_ url: String) {
        guard !asked.contains(url), AgentCards.isPicture(url), let target = URL(string: url) else { return }
        asked.insert(url)
        session.dataTask(with: target) { [weak self] data, response, _ in
            guard let data, (response as? HTTPURLResponse)?.statusCode == 200, let image = NSImage(data: data) else { return }
            DispatchQueue.main.async { self?.images[url] = image }
        }.resume()
    }
}
