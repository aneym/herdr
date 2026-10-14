import CoreGraphics

enum PaneGeometry {
    static func drawable(_ rect: CGRect) -> Bool {
        [rect.origin.x, rect.origin.y, rect.width, rect.height, rect.maxX, rect.maxY].allSatisfy(\.isFinite)
            && rect.size.width > 0 && rect.size.height > 0
    }

    static func scale(bounds: CGRect, area: CGRect) -> CGSize? {
        guard drawable(bounds), drawable(area) else { return nil }
        let scale = CGSize(width: bounds.width / area.width, height: bounds.height / area.height)
        guard scale.width.isFinite, scale.height.isFinite else { return nil }
        return scale
    }

    static func box(_ rect: CGRect, bounds: CGRect, area: CGRect) -> CGRect {
        guard drawable(rect), let scale = scale(bounds: bounds, area: area) else { return .zero }
        let gapX: CGFloat = rect.minX > area.minX ? 1 : 0
        let gapY: CGFloat = rect.minY > area.minY ? 1 : 0
        let box = CGRect(x: (rect.minX - area.minX) * scale.width + gapX,
                         y: (rect.minY - area.minY) * scale.height + gapY,
                         width: rect.width * scale.width - gapX, height: rect.height * scale.height - gapY)
        guard drawable(box) else { return .zero }
        let integral = box.integral
        return drawable(integral) ? integral : .zero
    }
}
