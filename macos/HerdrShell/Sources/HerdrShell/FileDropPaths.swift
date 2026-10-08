import Foundation

/// Finder URLs are absolute paths. Escape shell syntax without changing Unicode
/// or walking directories; each item remains one argument, ready for more input.
enum FileDropPaths {
    static func text(_ paths: [String]) -> String {
        let specials = CharacterSet(charactersIn: "\\\"'`$!&;()[]{}<>|*?~#")
            .union(.whitespacesAndNewlines)
        return paths.map { path in
            path.unicodeScalars.map { scalar in
                (specials.contains(scalar) ? "\\" : "") + String(scalar)
            }.joined() + " "
        }.joined()
    }
}
