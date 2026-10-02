import AppKit
import Foundation

// P18: factory model. Reads the overlay, box registry, pool state, disk watch,
// routing table, decider, workflow tabs, and the local pools API. The view only
// draws `FactorySnapshot`. Nothing here writes those sources.

struct FactorySources {
    var overlay: String
    var poolsURL: String
    var routing: String
    var boxes: String
    var poolState: String
    var disk: String
    var workflows: String
    var repo: String
    var decider: String
    var poolsInterval: TimeInterval

    static func resolved() -> FactorySources {
        func env(_ key: String, _ fallback: String) -> String {
            let raw = ProcessInfo.processInfo.environment[key] ?? ""
            let v = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            if v.isEmpty { return (fallback as NSString).expandingTildeInPath }
            return (v as NSString).expandingTildeInPath
        }
        let home = TerminalTheme.realHome()
        let interval = Double(ProcessInfo.processInfo.environment["FACTORY_POOLS_INTERVAL"] ?? "") ?? 60
        return FactorySources(
            overlay: env("FACTORY_OVERLAY", home + "/.agent-rails/herdr/overlay.json"),
            poolsURL: env("FACTORY_POOLS_URL", "http://127.0.0.1:2455/api/pools"),
            routing: env("FACTORY_ROUTING", home + "/.agent-lb/managed/coding-agents/routing-table.json"),
            boxes: env("FACTORY_BOXES", home + "/.agent-rails/factory/boxes.json"),
            poolState: env("FACTORY_POOLSTATE", home + "/.agent-rails/factory/state/pool.json"),
            disk: env("FACTORY_DISK", home + "/.agent-rails/fleet-disk-watch/state.json"),
            workflows: env("FACTORY_WORKFLOWS_DIR", home + "/.agent-rails/workflows/tabs"),
            repo: env("FACTORY_REPO", "/Volumes/StudioExt/repos/agent-rails"),
            decider: "/Volumes/StudioExt/repos/agent-lb/clients/open-factory/open_factory/decider.json",
            poolsInterval: interval > 0 ? interval : 60)
    }
}

enum FactoryText {
    static let tz = TimeZone(identifier: "America/New_York") ?? TimeZone(secondsFromGMT: -4 * 3600)!

    static func scrub(_ s: String) -> String {
        var out = s
        let patterns = [
            #"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#,
            #"\b(?:sk-|ghp_|gho_|github_pat_|xox[baprs]-|AKIA)[A-Za-z0-9_\-]{8,}"#,
            #"(?i)\b(?:acct|account)[_-][A-Za-z0-9]{4,}\b"#,
        ]
        for p in patterns {
            guard let re = try? NSRegularExpression(pattern: p) else { continue }
            let range = NSRange(out.startIndex..., in: out)
            out = re.stringByReplacingMatches(in: out, range: range, withTemplate: "[redacted]")
        }
        return out
    }

    static func ageWords(_ seconds: TimeInterval) -> String {
        let s = max(0, Int(seconds.rounded(.down)))
        if s < 60 { return "\(s) s" }
        if s < 3600 { return "\(s / 60) m" }
        if s < 86400 { return "\(s / 3600) h" }
        return "\(s / 86400) d"
    }

    static func flightAge(_ seconds: TimeInterval) -> String {
        let s = max(0, Int(seconds.rounded(.down)))
        if s < 60 { return "\(s)s" }
        if s < 3600 { return "\(s / 60)m" }
        if s < 86400 { return "\(s / 3600)h" }
        return "\(s / 86400)d"
    }

    static func clock(_ date: Date, format: String) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = tz
        f.dateFormat = format
        return f.string(from: date)
    }

    /// `resets 6:49 PM` today, `resets Fri 12 PM` on another day. Minutes drop out on the hour.
    static func reset(_ date: Date, now: Date) -> String {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = tz
        let minute = cal.component(.minute, from: date)
        let time = clock(date, format: minute == 0 ? "h a" : "h:mm a")
        if cal.isDate(date, inSameDayAs: now) { return "resets \(time)" }
        return "resets \(clock(date, format: "EEE")) \(time)"
    }

    static func percent(_ value: Double) -> String {
        if abs(value - value.rounded()) < 0.05 { return String(format: "%.0f%%", value) }
        return String(format: "%.1f%%", locale: Locale(identifier: "en_US_POSIX"), value)
    }
}

struct MachineRow: Equatable, Identifiable {
    var name: String
    var kind: String
    var summary: String
    var slots: String
    var disk: String
    var state: String
    var attention: String
    var dimmed: Bool
    var usageState: String = ""
    var usageLine: String = ""
    var id: String { name }
}

struct HostUsage: Equatable {
    var state: String
    var loadPerCore: Double?
    var cpuPct: Int?
    var memUsed: Int?
    var memTotal: Int?
    var slotsUsed: Int?
    var slotsTotal: Int?
    var age: Int?

    var visible: Bool { if let age { return age <= 30 }; return false }

    var line: String {
        guard visible else { return "" }
        var parts: [String] = []
        if let loadPerCore { parts.append(String(format: "%.2f/core", locale: Locale(identifier: "en_US_POSIX"), loadPerCore)) }
        if let cpuPct { parts.append("\(cpuPct)%") }
        if let memUsed, let memTotal { parts.append("\(memUsed)/\(memTotal) MB") }
        if let slotsUsed, let slotsTotal { parts.append("\(slotsUsed)/\(slotsTotal)") }
        return parts.joined(separator: "  ")
    }
}

struct PoolRow: Equatable, Identifiable {
    var id: String
    var provider: String
    var counts: String
    var fiveHour: Double?
    var fiveHourLabel: String
    var weekly: Double?
    var weeklyLabel: String
    var pace: String
    var monthly: String
    var refill: String
    var tone: String
}

struct RouteRow: Equatable, Identifiable {
    var name: String
    var chips: [String]
    var expanded: Bool
    var pick: String
    var id: String { name }
}

struct FlightRow: Equatable, Identifiable {
    var id: String
    var name: String
    var lane: String
    var tab: String
    var age: String
    var host: String
    var headless: Bool
}

struct LandedRow: Equatable, Identifiable {
    var id: String
    var time: String
    var subject: String
}

struct FactorySnapshot: Equatable {
    var updated: String = "updated —"
    var machines: [MachineRow] = []
    var poolsAge: String = ""
    var poolsAgeSeconds: Double?
    var poolsStale: Bool = false
    var pools: [PoolRow] = []
    var ladderMode: String = ""
    var routes: [RouteRow] = []
    var decider: String = ""
    var flights: [FlightRow] = []
    var landedCount: Int = 0
    var landed: [LandedRow] = []

    func jsonObject() -> [String: Any] {
        [
            "updated": updated,
            "machines": machines.map {
                ["name": $0.name, "kind": $0.kind, "summary": $0.summary, "slots": $0.slots,
                 "disk": $0.disk, "state": $0.state, "attention": $0.attention, "dimmed": $0.dimmed] as [String: Any]
            },
            "pools_age_s": poolsAgeSeconds ?? NSNull(),
            "pools_stale": poolsStale,
            "pools": pools.map {
                ["id": $0.id, "provider": $0.provider, "counts": $0.counts,
                 "five_hour": $0.fiveHour ?? NSNull(), "five_hour_label": $0.fiveHourLabel,
                 "weekly": $0.weekly ?? NSNull(), "weekly_label": $0.weeklyLabel,
                 "pace": $0.pace, "monthly": $0.monthly, "refill": $0.refill, "tone": $0.tone] as [String: Any]
            },
            "ladder_mode": ladderMode,
            "routing": routes.map {
                ["name": $0.name, "chips": $0.chips, "expanded": $0.expanded, "pick": $0.pick] as [String: Any]
            },
            "decider": decider,
            "flights": flights.map {
                ["id": $0.id, "name": $0.name, "lane": $0.lane, "tab": $0.tab,
                 "age": $0.age, "host": $0.host, "headless": $0.headless] as [String: Any]
            },
            "landed_count": landedCount,
            "landed": landed.map { ["time": $0.time, "subject": $0.subject] as [String: Any] },
        ]
    }
}

private struct HostInfo {
    var name: String
    var summary: String
    var attention: String
    var usage: HostUsage?
}

private struct BoxInfo {
    var name: String
    var kind: String
    var sessions: Int?
    var checks: Int?
    var enabled: Bool
}

private struct PoolBox {
    var drained: Set<String>
    var drainedWhy: [String: String]
    var downUntil: [String: Date]
    var downWhy: [String: String]
}

private struct DiskInfo {
    var status: String
    var held: Bool
    var free: Double?
    var at: Date?
}

private struct PoolSrc {
    var id: String
    var provider: String
    var usable: Int?
    var total: Int?
    var headroom: Double?
    var fiveHour: Double?
    var weekly: Double?
    var pace: Double?
    var monthly: Double?
    var fiveReset: Date?
    var weeklyReset: Date?
    var cycleReset: Date?
    var refillAt: Date?
    var refillAccounts: Int?
}

private struct RouteSrc {
    var name: String
    var chips: [String]
}

private struct FlightSrc {
    var id: String
    var name: String
    var lane: String
    var tab: String
    var host: String
    var started: Date?
    var headless: Bool
}

private enum JSONBox {
    static func obj(_ v: Any?) -> [String: Any]? { v as? [String: Any] }
    static func list(_ v: Any?) -> [Any]? { v as? [Any] }
    static func str(_ v: Any?) -> String? {
        guard let s = v as? String else { return nil }
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? nil : t
    }
    static func num(_ v: Any?) -> Double? {
        switch v {
        case let n as NSNumber:
            if CFGetTypeID(n) == CFBooleanGetTypeID() { return nil }
            return n.doubleValue
        case let n as Double: return n
        case let n as Int: return Double(n)
        default: return nil
        }
    }
    static func int(_ v: Any?) -> Int? { num(v).map { Int($0.rounded()) } }
    static func bool(_ v: Any?, default def: Bool) -> Bool {
        switch v {
        case let b as Bool: return b
        case let n as NSNumber: return n.boolValue
        default: return def
        }
    }
    static func held(_ v: Any?) -> Bool {
        switch v {
        case nil, is NSNull: return false
        case let b as Bool: return b
        case let n as NSNumber:
            if CFGetTypeID(n) == CFBooleanGetTypeID() { return n.boolValue }
            return n.doubleValue != 0
        case let s as String:
            let t = s.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            return !t.isEmpty && t != "false" && t != "null" && t != "0"
        default: return false
        }
    }
    static func date(_ v: Any?) -> Date? {
        if let n = num(v) {
            let secs = n > 1_000_000_000_000 ? n / 1000 : n
            return Date(timeIntervalSince1970: secs)
        }
        guard let s = str(v) else { return nil }
        let iso = ISO8601DateFormatter()
        iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = iso.date(from: s) { return d }
        iso.formatOptions = [.withInternetDateTime]
        return iso.date(from: s)
    }
    static func reason(_ v: Any?) -> String {
        if let s = str(v) { return FactoryText.scrub(s) }
        if let d = obj(v), let s = str(d["reason"]) { return FactoryText.scrub(s) }
        return ""
    }
    static func read(_ path: String) -> Any? {
        guard let data = FileManager.default.contents(atPath: path) else { return nil }
        return try? JSONSerialization.jsonObject(with: data)
    }
    static func stamp(_ path: String) -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate]) as? Date
    }
    static func dirStamp(_ path: String) -> Date? {
        var latest = stamp(path)
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: path) else { return latest }
        for name in names where name.hasSuffix(".json") {
            guard let d = stamp((path as NSString).appendingPathComponent(name)) else { continue }
            if latest == nil || d > latest! { latest = d }
        }
        return latest
    }
}

private final class GitOut: @unchecked Sendable { var data = Data() }

private enum FactoryRead {
    static func hosts(_ path: String) -> (Date?, [String: HostInfo]) {
        let overlay = JSONBox.obj(JSONBox.read(path))
        var hosts: [String: HostInfo] = [:]
        for item in JSONBox.list(overlay?["hosts"]) ?? [] {
            guard let d = JSONBox.obj(item), let name = JSONBox.str(d["name"]) else { continue }
            let attention = JSONBox.str(d["attention"])?.lowercased() ?? ""
            hosts[name.lowercased()] = HostInfo(
                name: name,
                summary: FactoryText.scrub(JSONBox.str(d["summary"]) ?? ""),
                attention: attention == "warn" || attention == "act" ? attention : "",
                usage: Self.usage(JSONBox.obj(d["usage"])))
        }
        return (JSONBox.date(overlay?["generated_at"]), hosts)
    }

    static func usage(_ d: [String: Any]?) -> HostUsage? {
        guard let d else { return nil }
        return HostUsage(
            state: JSONBox.str(d["state"]) ?? "",
            loadPerCore: JSONBox.num(d["load_per_core"]),
            cpuPct: JSONBox.int(d["cpu_pct"]),
            memUsed: JSONBox.int(d["mem_used_mb"]),
            memTotal: JSONBox.int(d["mem_total_mb"]),
            slotsUsed: JSONBox.int(d["slots_used"]),
            slotsTotal: JSONBox.int(d["slots_total"]),
            age: JSONBox.int(d["age_s"]))
    }

    static func boxes(_ path: String) -> [String: BoxInfo] {
        var boxes: [String: BoxInfo] = [:]
        guard let root = JSONBox.obj(JSONBox.read(path)) else { return boxes }
        for item in JSONBox.list(root["boxes"]) ?? [] {
            guard let d = JSONBox.obj(item), let name = JSONBox.str(d["name"]) else { continue }
            boxes[name.lowercased()] = BoxInfo(
                name: name, kind: JSONBox.str(d["kind"]) ?? "",
                sessions: JSONBox.int(d["sessions"]), checks: JSONBox.int(d["checks"]),
                enabled: JSONBox.bool(d["enabled"], default: true))
        }
        return boxes
    }

    static func poolBoxes(_ path: String) -> PoolBox {
        let pool = JSONBox.obj(JSONBox.read(path))
        let drained = Set((JSONBox.list(pool?["drained"]) ?? []).compactMap { JSONBox.str($0)?.lowercased() })
        var drainedWhy: [String: String] = [:]
        for (k, v) in JSONBox.obj(pool?["drained_why"]) ?? [:] { drainedWhy[k.lowercased()] = JSONBox.reason(v) }
        var downUntil: [String: Date] = [:]
        for (k, v) in JSONBox.obj(pool?["down"]) ?? [:] {
            if let d = JSONBox.date(v) { downUntil[k.lowercased()] = d }
        }
        var downWhy: [String: String] = [:]
        for (k, v) in JSONBox.obj(pool?["down_why"]) ?? [:] { downWhy[k.lowercased()] = JSONBox.reason(v) }
        return PoolBox(drained: drained, drainedWhy: drainedWhy, downUntil: downUntil, downWhy: downWhy)
    }

    static func disk(_ path: String) -> [String: DiskInfo] {
        var out: [String: DiskInfo] = [:]
        guard let root = JSONBox.obj(JSONBox.read(path)) else { return out }
        for (k, v) in root {
            guard let d = JSONBox.obj(v) else { continue }
            let last = JSONBox.obj(d["last_run"])
            out[k.lowercased()] = DiskInfo(
                status: JSONBox.str(d["status"]) ?? "",
                held: JSONBox.held(d["held"]),
                free: JSONBox.num(last?["free_gib"]),
                at: JSONBox.date(last?["at"]))
        }
        return out
    }

    static func pools(_ data: Data) -> [PoolSrc]? {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        guard let list = JSONBox.list(root["pools"]), !list.isEmpty else { return nil }
        var out: [PoolSrc] = []
        for item in list {
            guard let d = JSONBox.obj(item), let id = JSONBox.str(d["id"]) else { continue }
            var refillAt: Date?
            var refillN: Int?
            if let first = (JSONBox.list(d["refills"]) ?? []).compactMap(JSONBox.obj).first {
                refillAt = JSONBox.date(first["at"])
                refillN = JSONBox.int(first["accounts"])
            }
            out.append(PoolSrc(
                id: id, provider: JSONBox.str(d["provider"]) ?? "",
                usable: JSONBox.int(d["usableAccounts"]), total: JSONBox.int(d["totalAccounts"]),
                headroom: JSONBox.num(d["headroomPercent"]),
                fiveHour: JSONBox.num(d["fiveHourRemainingPercent"]),
                weekly: JSONBox.num(d["weeklyRemainingPercent"]),
                pace: JSONBox.num(d["weeklyPacePercent"]),
                monthly: JSONBox.num(d["monthlyRemaining"]),
                fiveReset: JSONBox.date(d["fiveHourResetAt"]),
                weeklyReset: JSONBox.date(d["weeklyResetAt"]),
                cycleReset: JSONBox.date(d["cycleResetAt"]),
                refillAt: refillAt, refillAccounts: refillN))
        }
        return out
    }

    static func routing(_ s: FactorySources) -> (String, [RouteSrc], String) {
        let table = JSONBox.obj(JSONBox.read(s.routing)) ?? [:]
        let mode = JSONBox.str(table["ladder_mode"]) ?? JSONBox.str(table["ladder"]) ?? ""
        let chains = JSONBox.obj(table["interim_ladder"])
            ?? JSONBox.obj(JSONBox.obj(table["ladders"])?[mode])
            ?? [:]
        let order = ["implement", "mechanical", "explore", "review", "research", "verify", "plan", "computer", "council"]
        let names = chains.keys.sorted { a, b in
            let ia = order.firstIndex(of: a) ?? 99
            let ib = order.firstIndex(of: b) ?? 99
            if ia != ib { return ia < ib }
            return a < b
        }
        return (mode, names.map { RouteSrc(name: $0, chips: chips(chains[$0])) }, deciderLine(JSONBox.obj(JSONBox.read(s.decider))))
    }

    static func chips(_ value: Any?) -> [String] {
        if let list = JSONBox.list(value) { return list.compactMap(chip) }
        if let d = JSONBox.obj(value), let rungs = JSONBox.obj(d["rungs"]) {
            return rungs.keys.sorted().map { chip(rungs[$0]) ?? $0 }
        }
        return []
    }

    static func chip(_ value: Any?) -> String? {
        if let s = JSONBox.str(value) { return s }
        guard let d = JSONBox.obj(value) else { return nil }
        if let label = JSONBox.str(d["label"]) ?? JSONBox.str(d["chip"]) { return label }
        let model = JSONBox.str(d["model"]) ?? JSONBox.str(d["id"]) ?? ""
        if model.isEmpty { return nil }
        if let effort = JSONBox.str(d["effort"]), !effort.isEmpty, !model.hasSuffix(effort) {
            return "\(model)-\(effort)"
        }
        return model
    }

    static func deciderLine(_ d: [String: Any]?) -> String {
        guard let d else { return "" }
        let who = JSONBox.str(d["decider"]) ?? ""
        let policy = JSONBox.str(d["policy"]) ?? JSONBox.str(d["policy_name"]) ?? ""
        let version = JSONBox.int(d["version"]).map(String.init) ?? JSONBox.str(d["version"]) ?? ""
        if who.isEmpty && version.isEmpty { return "" }
        let policyBit = policy.isEmpty ? "" : " (\(policy))"
        let ver = version.isEmpty ? "" : ", decider.json v\(version)"
        return "Decider: \(who)\(policyBit)\(ver)"
    }

    static func flights(_ dir: String) -> [FlightSrc] {
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: dir) else { return [] }
        var out: [FlightSrc] = []
        for name in names where name.hasSuffix(".json") {
            guard let d = JSONBox.obj(JSONBox.read((dir as NSString).appendingPathComponent(name))) else { continue }
            let runner = JSONBox.int(d["runner_pid"]) ?? 0
            let child = JSONBox.int(d["child_pid"])
            if !pidAlive(runner) && !(child != nil && pidAlive(child!)) { continue }
            let id = JSONBox.str(d["run_id"]) ?? name
            out.append(FlightSrc(
                id: id,
                name: FactoryText.scrub(JSONBox.str(d["name"]) ?? id),
                lane: JSONBox.str(d["lane"]) ?? "",
                tab: JSONBox.str(d["tab"]) ?? "",
                host: JSONBox.str(d["host"]) ?? "",
                started: JSONBox.date(d["started"]),
                headless: JSONBox.bool(d["headless"], default: false)))
        }
        out.sort { ($0.started ?? .distantPast) > ($1.started ?? .distantPast) }
        return out
    }

    static func pidAlive(_ pid: Int) -> Bool {
        if pid <= 0 { return false }
        return kill(Int32(pid), 0) == 0
    }

    static func landed(_ repo: String) -> (Int, [LandedRow]) {
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        proc.arguments = ["--no-pager", "-C", repo, "log", "origin/main", "--since=midnight", "--format=%h%x09%ct%x09%s"]
        let pipe = Pipe()
        proc.standardOutput = pipe
        proc.standardError = FileHandle.nullDevice
        let box = GitOut()
        let reader = DispatchQueue(label: "herdr.factory.git")
        reader.async { box.data = pipe.fileHandleForReading.readDataToEndOfFile() }
        do { try proc.run() } catch { return (0, []) }
        let deadline = Date().addingTimeInterval(15)
        while proc.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        if proc.isRunning { proc.terminate() }
        reader.sync {}
        let text = String(data: box.data, encoding: .utf8) ?? ""
        var rows: [(Date, String, String)] = []
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false).map(String.init)
            guard parts.count == 3, let secs = Double(parts[1]) else { continue }
            rows.append((Date(timeIntervalSince1970: secs), parts[0], FactoryText.scrub(parts[2])))
        }
        let shown = rows.prefix(5).map {
            LandedRow(id: $0.1, time: FactoryText.clock($0.0, format: "h:mm a"), subject: $0.2)
        }
        return (rows.count, Array(shown))
    }
}

final class FactoryModel: ObservableObject {
    @Published private(set) var snapshot = FactorySnapshot()
    let sources: FactorySources
    let dumpPath: String?
    var onChange: (() -> Void)?

    private let io = DispatchQueue(label: "herdr.factory.io", qos: .utility, attributes: .concurrent)
    private var timers: [Timer] = []
    private var generation: [String: Int] = [:]
    private var seen: [String: Date] = [:]
    private var loadedAt: [String: Date] = [:]
    private var primed = false
    private var hosts: [String: HostInfo] = [:]
    private var generatedAt: Date?
    private var boxes: [String: BoxInfo] = [:]
    private var poolBox = PoolBox(drained: [], drainedWhy: [:], downUntil: [:], downWhy: [:])
    private var disk: [String: DiskInfo] = [:]
    private var poolSources: [PoolSrc] = []
    private var poolsFetched: Date?
    private var poolsStale = false
    private var routeSources: [RouteSrc] = []
    private var ladderMode = ""
    private var decider = ""
    private var flightSources: [FlightSrc] = []
    private var landedCount = 0
    private var landed: [LandedRow] = []
    private var expanded: String?
    private var picks: [String: String] = [:]
    private var pickAt: [String: Date] = [:]
    private let poolSession: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.timeoutIntervalForRequest = 4
        config.timeoutIntervalForResource = 4
        return URLSession(configuration: config)
    }()
    private var routingLoaded = false
    private var flightsLoaded = false
    private var landedLoaded = false
    private var poolsTask: URLSessionDataTask?
    private var picking = Set<String>()
    private var watching = false

    init(sources: FactorySources = .resolved(), dumpPath: String? = nil) {
        self.sources = sources
        self.dumpPath = dumpPath
    }

    var routingReady: Bool { routingLoaded }
    var flightsReady: Bool { flightsLoaded }
    var landedReady: Bool { landedLoaded }

    func start() {
        reloadHosts()
        reloadBoxes()
        reloadPoolBoxes()
        reloadDisk()
        reloadRouting()
        reloadFlights()
        reloadLanded()
        fetchPools()
        let age = Timer(timeInterval: 1, repeats: true) { [weak self] _ in self?.tick() }
        let pools = Timer(timeInterval: sources.poolsInterval, repeats: true) { [weak self] _ in self?.fetchPools() }
        let git = Timer(timeInterval: 60, repeats: true) { [weak self] _ in self?.reloadLanded() }
        for t in [age, pools, git] {
            RunLoop.main.add(t, forMode: .common)
            timers.append(t)
        }
    }

    func stop() {
        timers.forEach { $0.invalidate() }
        timers.removeAll()
        poolsTask?.cancel()
    }

    func toggleRoute(_ name: String) {
        expanded = expanded == name ? nil : name
        publish()
        guard expanded == name, name == "implement" || name == "mechanical" else { return }
        if let at = pickAt[name], Date().timeIntervalSince(at) < 60, picks[name] != nil { return }
        runPick(name)
    }

    func openRoutingTable() {
        shellOpen(URL(fileURLWithPath: sources.routing))
    }

    private func tick() {
        let t0 = CFAbsoluteTimeGetCurrent()
        publish()
        log(String(format: "factory tick %.2f ms", (CFAbsoluteTimeGetCurrent() - t0) * 1000))
        guard primed, !watching else { return }
        watching = true
        let src = sources
        io.async { [weak self] in
            let stamps: [(String, Date?, TimeInterval)] = [
                ("hosts", JSONBox.stamp(src.overlay), 0),
                ("boxes", JSONBox.stamp(src.boxes), 15),
                ("pool", JSONBox.stamp(src.poolState), 15),
                ("disk", JSONBox.stamp(src.disk), 15),
                ("routing", JSONBox.stamp(src.routing), 15),
                ("decider", JSONBox.stamp(src.decider), 15),
                ("flights", JSONBox.dirStamp(src.workflows), 5),
            ]
            DispatchQueue.main.async {
                self?.watching = false
                self?.applyStamps(stamps)
            }
        }
    }

    private func applyStamps(_ stamps: [(String, Date?, TimeInterval)]) {
        for (key, stamp, limit) in stamps {
            guard let stamp else { continue }
            if seen[key] == nil {
                seen[key] = stamp
                continue
            }
            if seen[key] == stamp { continue }
            if limit > 0, let at = loadedAt[key], Date().timeIntervalSince(at) < limit { continue }
            seen[key] = stamp
            switch key {
            case "hosts": reloadHosts()
            case "boxes": reloadBoxes()
            case "pool": reloadPoolBoxes()
            case "disk": reloadDisk()
            case "routing", "decider": reloadRouting()
            case "flights": reloadFlights()
            default: break
            }
        }
    }

    private func bump(_ key: String) -> Int {
        let n = (generation[key] ?? 0) + 1
        generation[key] = n
        return n
    }

    private func reloadHosts() {
        let gen = bump("hosts")
        let path = sources.overlay
        io.async { [weak self] in
            let stamp = JSONBox.stamp(path)
            let parsed = FactoryRead.hosts(path)
            DispatchQueue.main.async {
                guard let self, self.generation["hosts"] == gen else { return }
                self.generatedAt = parsed.0
                self.hosts = parsed.1
                if let stamp { self.seen["hosts"] = stamp }
                self.loadedAt["hosts"] = Date()
                self.primed = true
                self.publish()
            }
        }
    }

    private func reloadBoxes() {
        let gen = bump("boxes")
        let path = sources.boxes
        io.async { [weak self] in
            let stamp = JSONBox.stamp(path)
            let parsed = FactoryRead.boxes(path)
            DispatchQueue.main.async {
                guard let self, self.generation["boxes"] == gen else { return }
                self.boxes = parsed
                if let stamp { self.seen["boxes"] = stamp }
                self.loadedAt["boxes"] = Date()
                self.publish()
            }
        }
    }

    private func reloadPoolBoxes() {
        let gen = bump("pool")
        let path = sources.poolState
        io.async { [weak self] in
            let stamp = JSONBox.stamp(path)
            let parsed = FactoryRead.poolBoxes(path)
            DispatchQueue.main.async {
                guard let self, self.generation["pool"] == gen else { return }
                self.poolBox = parsed
                if let stamp { self.seen["pool"] = stamp }
                self.loadedAt["pool"] = Date()
                self.publish()
            }
        }
    }

    private func reloadDisk() {
        let gen = bump("disk")
        let path = sources.disk
        io.async { [weak self] in
            let stamp = JSONBox.stamp(path)
            let parsed = FactoryRead.disk(path)
            DispatchQueue.main.async {
                guard let self, self.generation["disk"] == gen else { return }
                self.disk = parsed
                if let stamp { self.seen["disk"] = stamp }
                self.loadedAt["disk"] = Date()
                self.publish()
            }
        }
    }

    private func reloadRouting() {
        let gen = bump("routing")
        let src = sources
        io.async { [weak self] in
            let routingStamp = JSONBox.stamp(src.routing)
            let deciderStamp = JSONBox.stamp(src.decider)
            let parsed = FactoryRead.routing(src)
            DispatchQueue.main.async {
                guard let self, self.generation["routing"] == gen else { return }
                self.ladderMode = parsed.0
                self.routeSources = parsed.1
                self.decider = parsed.2
                self.routingLoaded = true
                if let routingStamp { self.seen["routing"] = routingStamp }
                if let deciderStamp { self.seen["decider"] = deciderStamp }
                self.loadedAt["routing"] = Date()
                self.loadedAt["decider"] = Date()
                self.publish()
            }
        }
    }

    private func reloadFlights() {
        let gen = bump("flights")
        let dir = sources.workflows
        io.async { [weak self] in
            let stamp = JSONBox.dirStamp(dir)
            let parsed = FactoryRead.flights(dir)
            DispatchQueue.main.async {
                guard let self, self.generation["flights"] == gen else { return }
                self.flightSources = parsed
                self.flightsLoaded = true
                if let stamp { self.seen["flights"] = stamp }
                self.loadedAt["flights"] = Date()
                self.publish()
            }
        }
    }

    private func reloadLanded() {
        let gen = bump("landed")
        let repo = sources.repo
        io.async { [weak self] in
            let parsed = FactoryRead.landed(repo)
            DispatchQueue.main.async {
                guard let self, self.generation["landed"] == gen else { return }
                self.landedCount = parsed.0
                self.landed = parsed.1
                self.landedLoaded = true
                self.publish()
            }
        }
    }

    private func fetchPools() {
        if poolsTask != nil { return }
        guard let url = URL(string: sources.poolsURL) else { return }
        let req = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 4)
        let task = poolSession.dataTask(with: req) { [weak self] data, response, _ in
            let ok = (response as? HTTPURLResponse).map { (200..<300).contains($0.statusCode) } ?? false
            let parsed = ok ? data.flatMap { FactoryRead.pools($0) } : nil
            DispatchQueue.main.async {
                guard let self else { return }
                self.poolsTask = nil
                if let parsed {
                    self.poolSources = parsed
                    self.poolsFetched = Date()
                    self.poolsStale = false
                } else {
                    self.poolsStale = true
                }
                self.publish()
            }
        }
        poolsTask = task
        task.resume()
    }

    private func runPick(_ name: String) {
        if picking.contains(name) { return }
        picking.insert(name)
        io.async { [weak self] in
            let text = Self.pick(name)
            DispatchQueue.main.async {
                guard let self else { return }
                self.picking.remove(name)
                self.picks[name] = text
                self.pickAt[name] = Date()
                self.publish()
            }
        }
    }

    private static func pick(_ name: String) -> String {
        let proc = Process()
        let route = (TerminalTheme.realHome() as NSString).appendingPathComponent(".local/bin/route")
        proc.executableURL = URL(fileURLWithPath: route)
        proc.arguments = ["pick", name]
        let out = Pipe()
        let err = Pipe()
        proc.standardOutput = out
        proc.standardError = err
        do { try proc.run() } catch { return "route pick failed" }
        let deadline = Date().addingTimeInterval(5)
        while proc.isRunning && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        let finished = !proc.isRunning
        if !finished { proc.terminate() }
        let stdout: String
        let stderr: String
        if finished {
            stdout = String(data: out.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            stderr = String(data: err.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        } else {
            stdout = ""
            stderr = ""
        }
        let text = stdout.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? stderr : stdout
        return FactoryText.scrub(text.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private func publish() {
        let now = Date()
        var snap = FactorySnapshot()
        if let generatedAt {
            snap.updated = "updated \(FactoryText.ageWords(now.timeIntervalSince(generatedAt))) ago"
        }
        snap.machines = composedMachines(now: now)
        if let poolsFetched {
            let age = now.timeIntervalSince(poolsFetched)
            snap.poolsAgeSeconds = age
            snap.poolsAge = "\(FactoryText.ageWords(age)) ago"
        }
        snap.poolsStale = poolsStale
        snap.pools = poolSources.map { format($0, now: now) }
        snap.ladderMode = ladderMode
        snap.routes = routeSources.map {
            RouteRow(name: $0.name, chips: $0.chips, expanded: expanded == $0.name, pick: picks[$0.name] ?? "")
        }
        snap.decider = decider
        snap.flights = flightSources.map { format($0, now: now) }
        snap.landedCount = landedCount
        snap.landed = landed
        snapshot = snap
        writeDump(snap)
        onChange?()
    }

    private func composedMachines(now: Date) -> [MachineRow] {
        let keys = Set(hosts.keys).union(boxes.keys)
        var rows: [MachineRow] = []
        for key in keys {
            let host = hosts[key]
            let box = boxes[key]
            let name = box?.name ?? host?.name ?? key
            let until = poolBox.downUntil[key]
            let downFuture = until.map { $0 > now } ?? false
            var state = "up"
            var why = ""
            if downFuture {
                state = "down until \(FactoryText.clock(until!, format: "HH:mm"))"
                why = poolBox.downWhy[key] ?? ""
            } else if disk[key]?.held == true {
                state = "held"
            } else if poolBox.drained.contains(key) {
                state = "drained"
                why = poolBox.drainedWhy[key] ?? ""
            }
            if !why.isEmpty { state += " (\(why))" }
            var slots = ""
            if let sessions = box?.sessions, let checks = box?.checks { slots = "\(sessions)/\(checks)" }
            var diskText = ""
            if let info = disk[key] {
                if let free = info.free {
                    diskText = String(format: "%.1f GiB", locale: Locale(identifier: "en_US_POSIX"), free)
                    if !info.status.isEmpty { diskText += " · \(info.status)" }
                    if let at = info.at { diskText += " · \(FactoryText.ageWords(now.timeIntervalSince(at))) ago" }
                } else if !info.status.isEmpty {
                    diskText = info.status
                }
            }
            let usage = host?.usage
            let show = usage?.visible == true
            rows.append(MachineRow(
                name: name, kind: box?.kind ?? "", summary: host?.summary ?? "", slots: slots, disk: diskText,
                state: state, attention: host?.attention ?? "", dimmed: box?.enabled == false,
                usageState: show ? (usage?.state ?? "") : "",
                usageLine: usage?.line ?? ""))
        }
        rows.sort { machineLess($0.name, $1.name) }
        return rows
    }

    private func machineLess(_ a: String, _ b: String) -> Bool {
        func r(_ name: String) -> (Int, String) {
            let n = name.lowercased()
            let g: Int
            if n == "studio" { g = 0 }
            else if n == "pc" || n.hasPrefix("pc-") { g = 1 }
            else if n == "ax42" { g = 2 }
            else if n.hasPrefix("forge") { g = 3 }
            else if n.contains("macbook") { g = 4 }
            else { g = 5 }
            return (g, n)
        }
        let ra = r(a), rb = r(b)
        if ra.0 != rb.0 { return ra.0 < rb.0 }
        return ra.1 < rb.1
    }

    private func format(_ p: PoolSrc, now: Date) -> PoolRow {
        let counts: String
        if let usable = p.usable, let total = p.total { counts = "\(usable)/\(total)" }
        else if let usable = p.usable { counts = "\(usable)" }
        else { counts = "" }
        let tone: String
        if p.usable == 0 { tone = "red" }
        else if let h = p.headroom, h < 15 { tone = "amber" }
        else { tone = "ok" }
        let five = p.fiveHour.map { FactoryText.percent($0) + (p.fiveReset.map { " · \(FactoryText.reset($0, now: now))" } ?? "") } ?? ""
        let week = p.weekly.map { FactoryText.percent($0) + (p.weeklyReset.map { " · \(FactoryText.reset($0, now: now))" } ?? "") } ?? ""
        let pace = p.pace.map { "pace \(FactoryText.percent($0))" } ?? ""
        var monthly = ""
        if let m = p.monthly {
            monthly = "monthly \(FactoryText.percent(m))"
            if let cycle = p.cycleReset { monthly += " · \(FactoryText.reset(cycle, now: now))" }
        }
        var refill = ""
        if let n = p.refillAccounts {
            refill = "+\(n) \(n == 1 ? "account" : "accounts")"
            if let at = p.refillAt { refill += " \(FactoryText.clock(at, format: "h:mm a"))" }
        }
        return PoolRow(id: p.id, provider: p.provider, counts: counts, fiveHour: p.fiveHour, fiveHourLabel: five,
                       weekly: p.weekly, weeklyLabel: week, pace: pace, monthly: monthly, refill: refill, tone: tone)
    }

    private func format(_ f: FlightSrc, now: Date) -> FlightRow {
        let age = f.started.map { FactoryText.flightAge(now.timeIntervalSince($0)) } ?? ""
        return FlightRow(id: f.id, name: f.name, lane: f.lane, tab: f.tab, age: age, host: f.host, headless: f.headless)
    }

    private func writeDump(_ snap: FactorySnapshot) {
        guard let dumpPath else { return }
        guard let data = try? JSONSerialization.data(withJSONObject: snap.jsonObject(), options: [.prettyPrinted, .sortedKeys]) else { return }
        try? data.write(to: URL(fileURLWithPath: dumpPath), options: .atomic)
    }
}
