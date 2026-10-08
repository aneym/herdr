import Foundation

// Golden cases protect WCAG luminance, invalid input, and both generated sidebar palettes.
@main
struct AreaContrastCheck {
    static func main() {
        let cases: [(String, UInt32, Bool)] = [
            ("#1F1F23", ChromePalette.dark.panel, true),
            ("#4F5BD5", ChromePalette.dark.panel, false),
            ("#FFFFFF", ChromePalette.light.panel, true),
            ("#4F5BD5", ChromePalette.light.panel, false),
            ("#1f1f23", ChromePalette.dark.panel, true),
            ("invalid", ChromePalette.dark.panel, false),
        ]
        for (color, background, expected) in cases {
            precondition(areaDotNeedsRing(color, background: background) == expected, "area dot contrast: \(color)")
        }
        print("area dot contrast: 6/6 passed")
    }
}
