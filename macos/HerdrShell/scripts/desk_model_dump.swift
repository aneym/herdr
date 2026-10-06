
// Golden-table boundary for the pure desk transition algorithms; no runtime mocks.
struct CaseInput: Decodable {
    let name: String
    let previous: [String: Set<String>]?
    let current: [String: DeskInfo]
    let previousFront: String?
    let active: String?
    let tab: String
}
let input = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let cases = try JSONDecoder().decode([CaseInput].self, from: input)
for c in cases {
    let value: [String: Any] = ["name": c.name, "landed": landed(previous: c.previous, current: c.current).sorted(),
                              "active": deskFront(previousFront: c.previousFront, current: c.current[c.tab] ?? .empty, active: c.active) as Any? ?? NSNull()]
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}
