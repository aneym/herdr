import Foundation

/// Link targets are never changed when adding wrap opportunities to their display text.
enum ChatLinks {
    private static let matcher = try? NSRegularExpression(pattern: #"\bhttps?://[^\s<>"']+"#, options: .caseInsensitive)

    static func plain(_ text: String) -> AttributedString { linkify(AttributedString(text)) }

    static func markdown(_ text: String) -> AttributedString {
        let parsed = (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
        return linkify(parsed)
    }

    static func linkify(_ input: AttributedString) -> AttributedString {
        var output = AttributedString()
        for run in input.runs {
            let text = String(input[run.range].characters)
            if run.link != nil {
                output.append(AttributedString(display(text), attributes: run.attributes))
                continue
            }
            var cursor = text.startIndex
            for match in matcher?.matches(in: text, range: NSRange(text.startIndex..., in: text)) ?? [] {
                guard let range = Range(match.range, in: text) else { continue }
                let candidate = trimmed(String(text[range]))
                guard let url = URL(string: candidate), let scheme = url.scheme?.lowercased(),
                      ["http", "https"].contains(scheme), url.host?.isEmpty == false else { continue }
                let end = text.index(range.lowerBound, offsetBy: candidate.count)
                output.append(AttributedString(String(text[cursor..<range.lowerBound]), attributes: run.attributes))
                var linked = AttributedString(display(candidate), attributes: run.attributes)
                linked.link = url
                output.append(linked)
                cursor = end
            }
            output.append(AttributedString(String(text[cursor...]), attributes: run.attributes))
        }
        return output
    }

    private static func trimmed(_ candidate: String) -> String {
        var value = candidate
        let pairs: [Character: Character] = [")": "(", "]": "[", "}": "{"]
        while let last = value.last, ".,;:!?)]}>'\"".contains(last) {
            if let open = pairs[last], value.filter({ $0 == open }).count >= value.filter({ $0 == last }).count { break }
            value.removeLast()
        }
        return value
    }

    private static func display(_ text: String) -> String {
        guard text.count > 80 else { return text }
        var result = "", unbroken = 0
        for char in text {
            result.append(char)
            unbroken += 1
            // Also break long path/query components that contain no punctuation.
            if "/?&=-_.".contains(char) || unbroken == 32 {
                result.append("\u{200B}")
                unbroken = 0
            }
        }
        return result
    }
}
