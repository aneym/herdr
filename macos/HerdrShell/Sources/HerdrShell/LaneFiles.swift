import Foundation

/// Paths captured in main before `HERDR_*` is cleared. Defaults are under the process home,
/// which for a lab launch is the lab home, so a check's fixtures only apply when it sets the env.
enum ShellPaths {
    static var lanes = ""
    static var areas = ""
    /// herdr-control's modes file (`herdr-lane park|unpark` writes it). `CONTROL_MODES` overrides it,
    /// the same variable lane.js reads, so a lab and its lane tool share one file.
    static var modes = ""

    /// areas.json sits beside the overlay, as the Rust server finds it (factory_overlay.rs:
    /// `path.with_file_name("areas.json")`), so a custom `FACTORY_OVERLAY` moves both files.
    /// `HERDR_AREAS_PATH` still names a fixture outright.
    static func configure(env: [String: String], home: String) {
        let overlay = env["FACTORY_OVERLAY"].map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .flatMap { $0.isEmpty ? nil : ($0 as NSString).expandingTildeInPath }
        lanes = env["HERDR_LANES_PATH"] ?? (home + "/.agent-rails/herdr/lanes.json")
        areas = env["HERDR_AREAS_PATH"]
            ?? overlay.map { (($0 as NSString).deletingLastPathComponent as NSString).appendingPathComponent("areas.json") }
            ?? (home + "/.agent-rails/herdr/areas.json")
        modes = env["CONTROL_MODES"] ?? (home + "/.agent-rails/herdr/modes.json")
    }
    static var filesPresent: Bool {
        FileManager.default.fileExists(atPath: lanes) || FileManager.default.fileExists(atPath: areas)
    }
}

struct LaneRecord: Equatable {
    var tab: String
    var name: String
    var label: String
    var kind: String?
    var goal: String?
    var goalArea: String?
    var section: String?
    var scopeURL: String?
    var reviewURL: String?
}

/// A parked tab: out of every filter but Parked, resumable in one click.
struct ParkRecord: Equatable {
    var note: String?
    var at: Date?
    var by: String?
}

struct AreaDef: Equatable, Identifiable {
    var id: String
    var name: String
    var color: String
}

struct TabAssign: Equatable {
    var area: String?
    var role: String?
    var name: String?
}

/// lanes.json + areas.json, re-read when either mtime changes.
struct LaneSnapshot: Equatable {
    var areas: [AreaDef] = []
    var lanes: [String: LaneRecord] = [:]
    var tabs: [String: TabAssign] = [:]
    var spaces: [String: String] = [:]
    var goalArea: [String: String] = [:]
    var goal: [String: String] = [:]
    /// `space_groups` from areas.json: sidebar groups of whole spaces, separate from Areas mode.
    /// nil when areas.json does not exist, so the overlay's own groups stand (Rust apply_space_groups).
    var spaceGroups: [Overlay.SpaceGroup]?
    var parked: [String: ParkRecord] = [:]
    var hasFiles = false

    static let empty = LaneSnapshot()

    func areaName(_ id: String) -> String {
        areas.first { $0.id == id }?.name ?? id
    }

    func areaColor(_ id: String) -> String {
        areas.first { $0.id == id }?.color ?? "#999999"
    }

    /// First match: tabs[tab].area, spaces[workspace], goal_area, goal, else unsorted.
    func areaId(tab: String, workspace: String, lane: LaneRecord?) -> String {
        if let a = tabs[tab]?.area, !a.isEmpty { return a }
        if let a = spaces[workspace], !a.isEmpty { return a }
        if let g = lane?.goalArea, let a = goalArea[g], !a.isEmpty { return a }
        if let g = lane?.goal, let a = goal[g], !a.isEmpty { return a }
        return "unsorted"
    }

    /// tabs[tab].role, else orchestrator when the lane kind is orchestrator, else project.
    func role(tab: String, lane: LaneRecord?) -> String {
        if let r = tabs[tab]?.role, !r.isEmpty { return r }
        if lane?.kind == "orchestrator" { return "orchestrator" }
        return "project"
    }

    /// tabs[tab].name, else the lane name with a leading `[scoping] ` stripped, else the herdr label.
    func displayName(tab: String, lane: LaneRecord?, fallback: String) -> String {
        if let n = tabs[tab]?.name, !n.isEmpty { return n }
        if let n = lane?.name, !n.isEmpty {
            let prefix = "[scoping] "
            return n.hasPrefix(prefix) ? String(n.dropFirst(prefix.count)) : n
        }
        return fallback
    }

    /// areas[] order, `unsorted` last. An area that is not listed still sorts after the listed ones
    /// and before unsorted.
    func orderedAreaIds(_ used: Set<String>) -> [String] {
        var listed = areas.map(\.id).filter { $0 != "unsorted" }
        for id in used.sorted() where id != "unsorted" && !listed.contains(id) { listed.append(id) }
        if used.contains("unsorted") || areas.contains(where: { $0.id == "unsorted" }) {
            listed.append("unsorted")
        }
        return listed
    }
}

final class LaneCatalog: ObservableObject {
    @Published private(set) var snapshot = LaneSnapshot.empty
    private var timer: Timer?
    private var stamp: [String: Date] = [:]

    func start() {
        reload()
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
            self?.reloadIfChanged()
        }
    }

    private func reloadIfChanged() {
        var next: [String: Date] = [:]
        var changed = false
        for path in [ShellPaths.lanes, ShellPaths.areas, ShellPaths.modes] where !path.isEmpty {
            let date = (try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as? Date) ?? Date.distantPast
            next[path] = date
            if stamp[path] != date { changed = true }
        }
        if changed { reload() }
    }

    func reload() {
        var snap = LaneSnapshot.empty
        snap.hasFiles = ShellPaths.filesPresent
        if let obj = Self.json(ShellPaths.lanes) {
            snap.lanes = Self.lanes(obj)
            // lanes.json carries the mode too, a writer tick behind; the modes file adds note and date.
            for item in obj["lanes"] as? [[String: Any]] ?? [] where Self.str(item["mode"]) == "parked" {
                if let tab = Self.str(item["tab"]) { snap.parked[tab] = ParkRecord() }
            }
        }
        if let obj = Self.json(ShellPaths.modes) { snap.parked = Self.parked(obj) }
        if let obj = Self.json(ShellPaths.areas) {
            snap.areas = Self.areas(obj)
            snap.tabs = Self.tabAssign(obj["tabs"] as? [String: Any] ?? [:])
            snap.spaces = Self.stringMap(obj["spaces"])
            snap.goalArea = Self.stringMap(obj["goal_area"])
            snap.goal = Self.stringMap(obj["goal"])
            // A `space_groups` Rust cannot parse (null, a non-list, a bad group) keeps the last ones too.
            snap.spaceGroups = Self.spaceGroups(obj) ?? snapshot.spaceGroups
        } else if FileManager.default.fileExists(atPath: ShellPaths.areas) {
            // Unreadable or half-written: keep the last parsed groups, as the Rust poller does.
            snap.spaceGroups = snapshot.spaceGroups
        }
        stamp[ShellPaths.lanes] = (try? FileManager.default.attributesOfItem(atPath: ShellPaths.lanes)[.modificationDate] as? Date) ?? Date.distantPast
        stamp[ShellPaths.areas] = (try? FileManager.default.attributesOfItem(atPath: ShellPaths.areas)[.modificationDate] as? Date) ?? Date.distantPast
        stamp[ShellPaths.modes] = (try? FileManager.default.attributesOfItem(atPath: ShellPaths.modes)[.modificationDate] as? Date) ?? Date.distantPast
        if snap != snapshot {
            snapshot = snap
            log("lanes: \(snap.lanes.count) areas: \(snap.areas.count) parked: \(snap.parked.count)")
        }
    }

    private static func json(_ path: String) -> [String: Any]? {
        guard !path.isEmpty, let data = FileManager.default.contents(atPath: path),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        return obj
    }

    /// As Rust apply_areas_file (serde, `#[serde(default)]`): a missing key or field takes its
    /// default and a group with a blank name is dropped, but a present value of the wrong type,
    /// null included, fails the whole document, which returns nil here.
    static func spaceGroups(_ obj: [String: Any]) -> [Overlay.SpaceGroup]? {
        guard let raw = obj["space_groups"] else { return [] }
        guard let list = raw as? [Any] else { return nil }
        var out: [Overlay.SpaceGroup] = []
        for item in list {
            guard let group = item as? [String: Any] else { return nil }
            let name = group["name"] ?? "", spaces = group["spaces"] ?? [Any]()
            guard let name = name as? String, let members = spaces as? [Any] else { return nil }
            var names: [String] = []
            for member in members {
                guard let member = member as? String else { return nil }
                names.append(member)
            }
            if !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { out.append(Overlay.SpaceGroup(name: name, spaces: names)) }
        }
        return out
    }

    private static func str(_ v: Any?) -> String? {
        guard let s = v as? String else { return nil }
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? nil : t
    }

    private static func lanes(_ obj: [String: Any]) -> [String: LaneRecord] {
        var out: [String: LaneRecord] = [:]
        for item in obj["lanes"] as? [[String: Any]] ?? [] {
            guard let tab = str(item["tab"]) else { continue }
            out[tab] = LaneRecord(tab: tab, name: str(item["name"]) ?? str(item["label"]) ?? tab,
                                  label: str(item["label"]) ?? "", kind: str(item["kind"]),
                                  goal: str(item["goal"]), goalArea: str(item["goal_area"]),
                                  section: str(item["section"]), scopeURL: str(item["scope_url"]),
                                  reviewURL: str(item["review_url"]))
        }
        return out
    }

    /// modes.json is the source of truth for parking: a tab is parked when its mode says so.
    private static func parked(_ obj: [String: Any]) -> [String: ParkRecord] {
        let iso = ISO8601DateFormatter()
        iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let plain = ISO8601DateFormatter()
        var out: [String: ParkRecord] = [:]
        for (tab, v) in obj["tabs"] as? [String: Any] ?? [:] {
            guard let d = v as? [String: Any], str(d["mode"]) == "parked" else { continue }
            let at = str(d["at"]).flatMap { iso.date(from: $0) ?? plain.date(from: $0) }
            out[tab] = ParkRecord(note: str(d["note"]), at: at, by: str(d["by"]))
        }
        return out
    }

    private static func areas(_ obj: [String: Any]) -> [AreaDef] {
        (obj["areas"] as? [[String: Any]] ?? []).compactMap { a in
            guard let id = str(a["id"]) else { return nil }
            return AreaDef(id: id, name: str(a["name"]) ?? id, color: str(a["color"]) ?? "#999999")
        }
    }

    private static func tabAssign(_ obj: [String: Any]) -> [String: TabAssign] {
        var out: [String: TabAssign] = [:]
        for (k, v) in obj {
            let d = v as? [String: Any] ?? [:]
            out[k] = TabAssign(area: str(d["area"]), role: str(d["role"]), name: str(d["name"]))
        }
        return out
    }

    private static func stringMap(_ v: Any?) -> [String: String] {
        var out: [String: String] = [:]
        for (k, val) in v as? [String: Any] ?? [:] {
            if let s = str(val) { out[k] = s }
        }
        return out
    }
}
