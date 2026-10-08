import Foundation
import CoreGraphics

enum PaneDropSide: String { case left, right, up, down }
enum PaneDropZone: Equatable {
    case centre(target: Int)
    case paneEdge(target: Int, side: PaneDropSide)
    case tabEdge(side: PaneDropSide)
}
struct PaneDropMetrics: Equatable {
    var tabEdge, bandMin, bandFraction, bandMaxFraction: CGFloat
    var gapReach: CGFloat = 1
}
enum PaneDrop {
    private static func contains(_ r: CGRect, _ p: CGPoint) -> Bool {
        p.x >= r.minX && p.x < r.maxX && p.y >= r.minY && p.y < r.maxY
    }
    private static func edge(_ r: CGRect, _ p: CGPoint, _ x: CGFloat, _ y: CGFloat) -> PaneDropSide? {
        let candidates: [(PaneDropSide, CGFloat, CGFloat)] = [
            (.left, p.x - r.minX, x), (.right, r.maxX - p.x, x),
            (.up, p.y - r.minY, y), (.down, r.maxY - p.y, y)]
        var result: PaneDropSide?, score = CGFloat.infinity
        for (side, distance, band) in candidates where distance < band {
            if distance / band < score { result = side; score = distance / band }
        }
        return result
    }
    static func zone(area: CGRect, panes: [CGRect], source: Int?, point: CGPoint, metrics: PaneDropMetrics) -> PaneDropZone? {
        guard contains(area, point) else { return nil }
        if panes.count >= 2, let side = edge(area, point, metrics.tabEdge, metrics.tabEdge) { return .tabEdge(side: side) }
        var nearest: (Int, CGRect, CGPoint)?, distance = CGFloat.infinity
        for (i, r) in panes.enumerated() where r.width > 0 && r.height > 0 {
            if contains(r, point) { nearest = (i, r, point); break }
            let clamped = CGPoint(x: min(max(point.x, r.minX), r.maxX), y: min(max(point.y, r.minY), r.maxY))
            let d = max(abs(point.x - clamped.x), abs(point.y - clamped.y))
            if d <= metrics.gapReach && d < distance { nearest = (i, r, clamped); distance = d }
        }
        guard let (i, r, p) = nearest, source != i else { return nil }
        func band(_ d: CGFloat) -> CGFloat { min(max(d * metrics.bandFraction, metrics.bandMin), d * metrics.bandMaxFraction) }
        if let side = edge(r, p, band(r.width), band(r.height)) { return .paneEdge(target: i, side: side) }
        return .centre(target: i)
    }
    static func estimateRect(area: CGRect, panes: [CGRect], zone: PaneDropZone) -> CGRect {
        let r: CGRect, side: PaneDropSide, share: CGFloat
        switch zone {
        case .centre(let target): return panes.indices.contains(target) ? panes[target] : .zero
        case .paneEdge(let target, let s):
            guard panes.indices.contains(target) else { return .zero }
            r = panes[target]; side = s; share = 0.5
        case .tabEdge(let s): r = area; side = s; share = 1 / 3
        }
        let firstShare = side == .left || side == .up ? share : 1 - share
        if side == .left || side == .right {
            let first = (r.width * firstShare).rounded(.toNearestOrAwayFromZero)
            return side == .left ? CGRect(x: r.minX, y: r.minY, width: first, height: r.height)
                : CGRect(x: r.minX + first, y: r.minY, width: r.width - first, height: r.height)
        }
        let first = (r.height * firstShare).rounded(.toNearestOrAwayFromZero)
        return side == .up ? CGRect(x: r.minX, y: r.minY, width: r.width, height: first)
            : CGRect(x: r.minX, y: r.minY + first, width: r.width, height: r.height - first)
    }
}
