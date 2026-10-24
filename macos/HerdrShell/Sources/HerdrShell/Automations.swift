import Foundation

struct AutomationSummary: Codable, Equatable {
    var id: String
    var name: String
    var cadence: String? = nil
    var status: String? = nil
    var result: String? = nil
    var last_run: String? = nil
    var next_run: String? = nil
    var report: String? = nil
    var report_url: String? = nil
}

enum AutomationRows {
    static func rows(_ list: [AutomationSummary]) -> [SpacesRow] {
        guard !list.isEmpty else { return [] }
        var out = [SpacesRow(id: "automations", kind: .section, title: "AUTOMATIONS")]
        for item in list {
            let state = mark(item.status)
            let hover = tooltip(item)
            var row = SpacesRow(id: "automation:" + item.id, kind: .automation, glyph: state.glyph, tone: state.tone,
                                title: item.name, trailing: trailing(item))
            row.hover = hover; row.report = item.report_url
            out.append(row)
            if let result = item.result, !result.isEmpty {
                var detail = SpacesRow(id: "automationresult:" + item.id, kind: .automation, depth: 1, title: result, dim: true)
                detail.hover = hover; detail.report = item.report_url
                out.append(detail)
            }
        }
        return out
    }

    static func mark(_ status: String?) -> (glyph: String, tone: String) {
        switch status {
        case "ok": return ("■", "done")
        case "acted": return ("■", "done")
        case "error": return ("■", "blocked")
        default: return ("●", "mute")
        }
    }

    static func shortCadence(_ cadence: String?) -> String? {
        guard let cadence = cadence?.trimmingCharacters(in: .whitespaces), !cadence.isEmpty else { return nil }
        let words = cadence.lowercased().split(separator: " ").map(String.init)
        guard words.count == 3, words[0] == "every", let n = Int(words[1]) else { return cadence }
        if words[2].hasPrefix("min") { return "\(n)m" }
        if words[2].hasPrefix("h") { return "\(n)h" }
        if words[2].hasPrefix("day") { return "\(n)d" }
        return cadence
    }

    static func clock(_ iso: String?) -> String? {
        guard let iso, let date = ISO8601DateFormatter().date(from: iso) else { return nil }
        let format = DateFormatter()
        format.dateFormat = "HH:mm"
        return format.string(from: date)
    }

    static func trailing(_ item: AutomationSummary) -> String {
        [shortCadence(item.cadence), clock(item.next_run).map { "next " + $0 }].compactMap { $0 }.joined(separator: " · ")
    }

    static func tooltip(_ item: AutomationSummary) -> String {
        [item.name, item.cadence, clock(item.last_run).map { "last " + $0 }, clock(item.next_run).map { "next " + $0 }, item.result]
            .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}
