import Foundation

struct DeskItem: Codable, Equatable {
    let id: String
    let kind: String
    let ref: String
    let title: String
    let mime: String
    let opened_by: String
    let opened_at_ms: UInt64
}

struct DeskInfo: Codable, Equatable {
    var items: [DeskItem]
    var front: String?
    static let empty = DeskInfo(items: [], front: nil)
}

func landed(previous: [String: Set<String>]?, current: [String: DeskInfo]) -> Set<String> {
    guard let previous else { return [] }
    return Set(current.compactMap { tab, desk in
        Set(desk.items.map(\.id)).subtracting(previous[tab] ?? []).isEmpty ? nil : tab
    })
}

func deskFront(previousFront: String?, current: DeskInfo, active: String?) -> String? {
    let ids = Set(current.items.map(\.id))
    if current.front != previousFront, let front = current.front, ids.contains(front) { return front }
    if let active, ids.contains(active) { return active }
    return current.front.flatMap { ids.contains($0) ? $0 : nil } ?? current.items.first?.id
}
