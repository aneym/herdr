import Foundation

enum ChannelKind: String { case prod, dev }

/// Prod is the app Alex opens. Dev is the lab bundle and any bare binary.
/// The two never share a defaults domain or an Application Support directory.
enum Channel {
    static let kind: ChannelKind = {
        switch Bundle.main.bundleIdentifier {
        case "com.aneyman.herdr-shell": return .prod
        default: return .dev
        }
    }()

    static var name: String { kind == .prod ? "Herdr Shell" : "Herdr Shell Dev" }

    static var home: String {
        let env = ProcessInfo.processInfo.environment["HOME"] ?? ""
        return env.isEmpty ? NSHomeDirectory() : env
    }

    static var lab: String { ProcessInfo.processInfo.environment["SHELL_LAB"] ?? "live" }

    /// Prod uses the bundle's own domain. Dev keeps a suite per lab, under a dev prefix.
    static var defaultsDomain: String {
        switch kind {
        case .prod: return "com.aneyman.herdr-shell"
        case .dev: return "herdr.shell.dev.\(lab)"
        }
    }

    static let store: UserDefaults = {
        switch kind {
        case .prod: return .standard
        case .dev: return UserDefaults(suiteName: defaultsDomain) ?? .standard
        }
    }()

    static var appSupport: URL {
        let folder = kind == .prod ? "HerdrShell" : "HerdrShell Dev"
        return URL(fileURLWithPath: home, isDirectory: true)
            .appendingPathComponent("Library/Application Support", isDirectory: true)
            .appendingPathComponent(folder, isDirectory: true)
    }

    static var installRoot: URL {
        URL(fileURLWithPath: home, isDirectory: true).appendingPathComponent("Applications", isDirectory: true)
    }

    static var commit: String {
        Bundle.main.object(forInfoDictionaryKey: "HerdrShellCommit") as? String ?? ""
    }

    static var builtAt: String {
        Bundle.main.object(forInfoDictionaryKey: "HerdrShellBuiltAt") as? String ?? ""
    }

    static let paneModesKey = "herdr.shell.paneModes"
    static let frameKey = "herdr.shell.windowFrame"
    static let detailRowKey = "herdr.shell.detailRow"
    static let dismissedKey = "herdr.shell.updateDismissed"

    static func paneModes() -> [String: String] {
        store.dictionary(forKey: paneModesKey) as? [String: String] ?? [:]
    }

    static func setPaneModes(_ modes: [String: String]) {
        store.set(modes, forKey: paneModesKey)
        store.synchronize()
    }

    static var sessionFile: URL { appSupport.appendingPathComponent("session.json") }

    static func printSelfTest() {
        let obj: [String: Any] = [
            "channel": kind.rawValue,
            "bundle_id": Bundle.main.bundleIdentifier ?? "",
            "name": name,
            "defaults_domain": defaultsDomain,
            "app_support": appSupport.path,
        ]
        let data = try! JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
    }
}

func shellResource(_ name: String) -> String {
    if let bundled = Bundle.main.resourceURL?.appendingPathComponent(name).path,
       FileManager.default.fileExists(atPath: bundled) {
        return bundled
    }
    return pkgRoot + "/Resources/" + name
}
