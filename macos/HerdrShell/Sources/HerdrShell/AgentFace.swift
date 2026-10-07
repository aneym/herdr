import AppKit
import SwiftUI

/// An AGENTS row's face in place of its state glyph: the agent's picture, else its initial on a tint
/// (Rails PersonFace). The state stays a small dot set into the lower right of the circle; an idle
/// agent shows none, and nothing here is a filled count or a red badge.
struct AgentFace: View {
    let face: SpacesRow.Face
    /// The row's tone (SpacesTree.mark): working, blocked, done, else quiet.
    let tone: String
    let t: Tokens
    @ObservedObject private var pictures = FacePictures.shared

    static let size: CGFloat = 16
    static let dot: CGFloat = 6
    /// Rails `TILE.wire` fills in personTint order: brun, rouge, orange, ambre, vert, turquoise, bleu, violet, rose.
    static let tints: [UInt32] = [0x8B5E3C, 0xE8483F, 0xF08A24, 0xF0B429, 0x3ECF8E, 0x2FBFA0, 0x3B93F0, 0x8B5CF6, 0xE152B0]

    var body: some View {
        circle
            .frame(width: Self.size, height: Self.size)
            .mask {
                // Cut a gap around the dot so it reads on any sidebar, glass or selection plate.
                ZStack {
                    Circle()
                    if dotColor != nil {
                        Circle().frame(width: Self.dot + 3, height: Self.dot + 3)
                            .offset(x: (Self.size - Self.dot) / 2 + 0.5, y: (Self.size - Self.dot) / 2 + 0.5)
                            .blendMode(.destinationOut)
                    }
                }.compositingGroup()
            }
            .overlay(alignment: .bottomTrailing) {
                if let dotColor {
                    Circle().fill(dotColor).frame(width: Self.dot, height: Self.dot).offset(x: 1, y: 1)
                }
            }
            .task(id: face.avatar) { if let url = face.avatar { pictures.load(url) } }
            .help(word.map { $0.prefix(1).uppercased() + $0.dropFirst() } ?? "")
    }

    @ViewBuilder private var circle: some View {
        if let url = face.avatar, let image = pictures.images[url] {
            Image(nsImage: image).resizable().interpolation(.high).aspectRatio(contentMode: .fill)
        } else {
            ZStack {
                Circle().fill(Color(hex: Self.tints[face.tint % Self.tints.count]).opacity(t.mode == .dark ? 0.32 : 0.24))
                Text(face.initial).font(.system(size: 9.5, weight: .semibold)).foregroundStyle(t.ink)
            }
        }
    }

    private var word: String? {
        switch tone {
        case "working": return "working"
        case "blocked": return "needs you"
        case "done": return "done"
        default: return nil
        }
    }

    /// Green while it works, blue when it needs you (the quiet "for you" dot), peach once done.
    private var dotColor: Color? {
        switch tone {
        case "working": return t.ok
        case "blocked": return t.accent
        case "done": return t.warn
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
