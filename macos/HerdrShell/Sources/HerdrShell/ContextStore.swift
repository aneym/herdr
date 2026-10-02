import Foundation

/// One file per tab: `HERDR_CONTEXT_DIR/<tab id with : as _>.json`.
enum ContextStore {
    struct Item: Equatable {
        var id: String
        var kind: String
        var title: String
        var ref: String
        var addedBy: String
        var addedAt: String
    }

    static var directory = ""

    static func load(tab: String) -> [Item] {
        guard let data = try? Data(contentsOf: file(tab)),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let arr = obj["items"] as? [[String: Any]] else { return [] }
        return arr.compactMap { d in
            guard let id = d["id"] as? String, let kind = d["kind"] as? String,
                  let title = d["title"] as? String, let ref = d["ref"] as? String else { return nil }
            return Item(id: id, kind: kind, title: title, ref: ref,
                        addedBy: d["added_by"] as? String ?? "agent",
                        addedAt: d["added_at"] as? String ?? "")
        }
    }

    static func add(tab: String, kind: String, title: String, ref: String, addedBy: String) {
        var items = load(tab: tab)
        let n = items.compactMap { Int($0.id.dropFirst()) }.max() ?? 0
        let stamp = ISO8601DateFormatter().string(from: Date())
        items.append(Item(id: "c\(n + 1)", kind: kind, title: title, ref: ref, addedBy: addedBy, addedAt: stamp))
        write(tab: tab, items: items)
    }

    static func remove(tab: String, id: String) {
        write(tab: tab, items: load(tab: tab).filter { $0.id != id })
    }

    static func mtime(tab: String) -> Date? {
        try? FileManager.default.attributesOfItem(atPath: file(tab).path)[.modificationDate] as? Date
    }

    private static func file(_ tab: String) -> URL {
        URL(fileURLWithPath: directory).appendingPathComponent(tab.replacingOccurrences(of: ":", with: "_") + ".json")
    }

    private static func write(tab: String, items: [Item]) {
        let dir = URL(fileURLWithPath: directory)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let arr: [[String: String]] = items.map {
            ["id": $0.id, "kind": $0.kind, "title": $0.title, "ref": $0.ref, "added_by": $0.addedBy, "added_at": $0.addedAt]
        }
        let obj: [String: Any] = ["version": 1, "tab": tab, "items": arr]
        guard let data = try? JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted, .sortedKeys]) else { return }
        try? data.write(to: file(tab), options: .atomic)
    }
}
