import CoreGraphics

@main
struct PaneGeometryTest {
    static func main() {
        let valid = CGRect(x: 0, y: 0, width: 100, height: 80)
        let invalid: [CGRect] = [
            .zero,
            CGRect(x: 0, y: 0, width: 0, height: 80),
            CGRect(x: 0, y: 0, width: 100, height: 0),
            CGRect(x: CGFloat.nan, y: 0, width: 100, height: 80),
            CGRect(x: 0, y: CGFloat.infinity, width: 100, height: 80),
            CGRect(x: 0, y: 0, width: CGFloat.nan, height: 80),
            CGRect(x: 0, y: 0, width: 100, height: CGFloat.infinity),
            CGRect(x: 0, y: 0, width: -1, height: 80)
        ]
        for rect in invalid {
            precondition(!PaneGeometry.drawable(rect))
            precondition(PaneGeometry.scale(bounds: valid, area: rect) == nil)
            precondition(PaneGeometry.scale(bounds: rect, area: valid) == nil)
            precondition(PaneGeometry.box(valid, bounds: valid, area: rect) == .zero)
            precondition(PaneGeometry.box(valid, bounds: rect, area: valid) == .zero)
            precondition(PaneGeometry.box(rect, bounds: valid, area: valid) == .zero)
        }
        precondition(PaneGeometry.box(valid, bounds: valid, area: valid) == valid)
        precondition(PaneGeometry.box(CGRect(x: 50, y: 40, width: 50, height: 40), bounds: valid, area: valid)
                     == CGRect(x: 51, y: 41, width: 49, height: 39))
        let huge = CGRect(x: 0, y: 0, width: CGFloat.greatestFiniteMagnitude, height: 80)
        let tiny = CGRect(x: 0, y: 0, width: CGFloat.leastNonzeroMagnitude, height: 80)
        precondition(PaneGeometry.scale(bounds: huge, area: tiny) == nil)
        print("PASS pane geometry: 8 invalid rectangles, overflow, normal layout and gap")
    }
}
