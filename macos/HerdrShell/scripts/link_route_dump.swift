import Foundation

/// Golden policy table: compile the production route without AppKit or an app.
@main
struct LinkRouteDump {
    static func main() {
        let urls = ["http://example.com", "https://example.com", "file:///tmp/link.txt",
                    "mailto:alex@example.com", "javascript:alert(1)", "ftp://example.com"]
        for raw in urls {
            guard let url = URL(string: raw) else { fatalError("Invalid fixture: \(raw)") }
            for shift in [false, true] {
                let route = LinkRoute.decide(url, shift: shift)
                print("\(url.scheme ?? "")|\(shift)|\(route)")
            }
        }
    }
}
