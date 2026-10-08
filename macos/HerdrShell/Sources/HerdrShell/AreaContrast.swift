import Foundation

/// WCAG contrast for an opaque data color against the sidebar's current palette.
func areaDotNeedsRing(_ color: String, background: UInt32) -> Bool {
    let hex = color.trimmingCharacters(in: .whitespacesAndNewlines)
    guard hex.count == 7, hex.hasPrefix("#"),
          hex.dropFirst().allSatisfy({ $0.isASCII && $0.isHexDigit }),
          let fill = UInt32(hex.dropFirst(), radix: 16) else { return false }
    func luminance(_ rgb: UInt32) -> Double {
        func linear(_ shift: UInt32) -> Double {
            let value = Double((rgb >> shift) & 0xff) / 255
            return value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0)
    }
    let a = luminance(fill), b = luminance(background)
    return (max(a, b) + 0.05) / (min(a, b) + 0.05) < 1.5
}
