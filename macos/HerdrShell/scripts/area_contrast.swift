import Foundation

// From the repository root: python3 macos/HerdrShell/scripts/check_area_contrast.py
// Golden cases protect WCAG luminance, invalid input, and both generated sidebar palettes.
// Theme.swift logs missing terminal themes when compiled outside the app.
func log(_ message: String) {
    FileHandle.standardError.write(Data((message + "\n").utf8))
}
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
