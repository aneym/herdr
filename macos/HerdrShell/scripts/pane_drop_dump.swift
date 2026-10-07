import CoreGraphics
import Foundation

/// Owner-written driver for scripts/check_pane_drop_zones.py (pane drag slice S6).
/// Compiled with Sources/HerdrShell/PaneDrop.swift only: no AppKit, no app, no server.
///
///   pane_drop_dump <shell/fixtures/pane-drop-zones.json> [--scale K]
///
/// Runs `PaneDrop.zone` and `PaneDrop.estimateRect` over every fixture case and prints one line
/// per case, in fixture order:
///
///   <index>|<zone>|<estimate>|<name>
///
///   zone      none | centre:<id> | pane_edge:<id>:<side> | tab_edge:<side>   (side: left|right|up|down)
///   estimate  x,y,w,h with three decimals, or "-" when the zone is none
///
/// `--scale K` multiplies the area, every rect, the point and the length metrics (tabEdge,
/// bandMin, gapReach) by K, as the Mac runs the same function in host points. The fractions
/// stay. This output format is the comparison contract; the implementer may not change it.
struct Fixture: Decodable {
    struct Metrics: Decodable { let tabEdge: Double; let bandMin: Double; let bandFraction: Double; let bandMaxFraction: Double }
    struct Pane: Decodable { let id: String; let rect: [Double] }
    struct Layout: Decodable { let area: [Double]; let panes: [Pane] }
    struct Case: Decodable { let name: String; let layout: String; let source: String?; let point: [Double] }
    let metrics: Metrics
    let layouts: [String: Layout]
    let cases: [Case]
}

@main
struct PaneDropDump {
    static func rect(_ v: [Double], _ k: Double) -> CGRect {
        CGRect(x: v[0] * k, y: v[1] * k, width: v[2] * k, height: v[3] * k)
    }

    static func main() throws {
        let args = CommandLine.arguments
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: args[1])))
        let k = args.firstIndex(of: "--scale").flatMap { Double(args[$0 + 1]) } ?? 1
        let m = fixture.metrics
        let metrics = PaneDropMetrics(tabEdge: CGFloat(m.tabEdge * k), bandMin: CGFloat(m.bandMin * k),
                                      bandFraction: CGFloat(m.bandFraction), bandMaxFraction: CGFloat(m.bandMaxFraction),
                                      gapReach: CGFloat(k))
        for (index, c) in fixture.cases.enumerated() {
            guard let layout = fixture.layouts[c.layout] else { fatalError("unknown layout \(c.layout)") }
            let area = rect(layout.area, k)
            let panes = layout.panes.map { rect($0.rect, k) }
            let source = c.source.flatMap { id in layout.panes.firstIndex { $0.id == id } }
            let point = CGPoint(x: c.point[0] * k, y: c.point[1] * k)
            let zone = PaneDrop.zone(area: area, panes: panes, source: source, point: point, metrics: metrics)
            var zoneText = "none"
            var estimateText = "-"
            if let zone {
                switch zone {
                case .centre(let target): zoneText = "centre:\(layout.panes[target].id)"
                case .paneEdge(let target, let side): zoneText = "pane_edge:\(layout.panes[target].id):\(side.rawValue)"
                case .tabEdge(let side): zoneText = "tab_edge:\(side.rawValue)"
                }
                let e = PaneDrop.estimateRect(area: area, panes: panes, zone: zone)
                estimateText = [e.minX, e.minY, e.width, e.height].map { String(format: "%.3f", Double($0)) }.joined(separator: ",")
            }
            print("\(index)|\(zoneText)|\(estimateText)|\(c.name)")
        }
    }
}
