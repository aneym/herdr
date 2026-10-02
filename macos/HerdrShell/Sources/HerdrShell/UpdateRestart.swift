import Foundation

struct StagedRelease {
    var commit: String
    var builtAt: String
    var ref: String
    var notes: [String]

    static func load(support: URL) -> StagedRelease? {
        let url = support.appendingPathComponent("staged.json")
        guard let data = try? Data(contentsOf: url),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let commit = obj["commit"] as? String, !commit.isEmpty else { return nil }
        let notes: [String]
        if let typed = obj["notes"] as? [String] {
            notes = typed
        } else if let any = obj["notes"] as? [Any] {
            notes = any.compactMap { $0 as? String }
        } else {
            notes = []
        }
        return StagedRelease(
            commit: commit,
            builtAt: obj["built_at"] as? String ?? "",
            ref: obj["ref"] as? String ?? "",
            notes: notes
        )
    }
}

struct UpdateOffer {
    var commit: String
    var builtAt: String
    var notes: [String]
    var failed: Bool
    var reason: String
}

enum Relaunch { case open, exec }

/// Swift's overlay hides `fork`. The C function is what a detached helper needs.
private func libcFork() -> Int32 {
    guard let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "fork") else { return -1 }
    let fn = unsafeBitCast(sym, to: (@convention(c) () -> Int32).self)
    return fn()
}

/// Swaps the staged app into an install root. The shell is a client; this path
/// only renames app bundles and starts the new one.
enum UpdateRestart {
    static let appName = "Herdr Shell.app"

    static func offer(support: URL, running: String, dismissed: String?) -> UpdateOffer? {
        guard let staged = StagedRelease.load(support: support), staged.commit != running else { return nil }
        if let dismissed, dismissed == staged.commit { return nil }
        let fail = readFailure(support: support)
        let failed = fail?.commit == staged.commit
        return UpdateOffer(
            commit: staged.commit,
            builtAt: staged.builtAt,
            notes: staged.notes,
            failed: failed,
            reason: failed ? (fail?.reason ?? "") : ""
        )
    }

    static func readFailure(support: URL) -> (commit: String, reason: String)? {
        let url = support.appendingPathComponent("update.log")
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        guard let line = text.split(separator: "\n", omittingEmptySubsequences: true).first else { return nil }
        let raw = String(line)
        guard raw.hasPrefix("failed ") else { return nil }
        let rest = raw.dropFirst("failed ".count)
        guard let space = rest.firstIndex(of: " ") else { return (String(rest), "") }
        let commit = String(rest[..<space])
        let reason = String(rest[rest.index(after: space)...])
        return (commit, reason)
    }

    static func writeFailure(support: URL, commit: String, reason: String) {
        try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        let line = "failed \(commit) \(reason)\n"
        try? Data(line.utf8).write(to: support.appendingPathComponent("update.log"))
    }

    static func executable(in app: URL) -> URL? {
        let mac = app.appendingPathComponent("Contents/MacOS")
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: mac.path) else { return nil }
        for name in names.sorted() {
            let url = mac.appendingPathComponent(name)
            if FileManager.default.isExecutableFile(atPath: url.path) { return url }
        }
        return nil
    }

    @discardableResult
    static func apply(installRoot: URL, support: URL, relaunch how: Relaunch) -> Bool {
        let fm = FileManager.default
        let app = installRoot.appendingPathComponent(appName)
        let previous = installRoot.appendingPathComponent(appName + ".previous")
        let staged = support.appendingPathComponent("staged").appendingPathComponent(appName)
        let stagedCommit = StagedRelease.load(support: support)?.commit ?? ""
        do {
            if fm.fileExists(atPath: previous.path) { try fm.removeItem(at: previous) }
            if fm.fileExists(atPath: app.path) { try fm.moveItem(at: app, to: previous) }
            guard fm.fileExists(atPath: staged.path) else {
                throw NSError(domain: "HerdrShellUpdate", code: 1, userInfo: [NSLocalizedDescriptionKey: "staged app is missing"])
            }
            try fm.moveItem(at: staged, to: app)
            guard executable(in: app) != nil else {
                throw NSError(domain: "HerdrShellUpdate", code: 2, userInfo: [NSLocalizedDescriptionKey: "staged app is corrupt"])
            }
            try? fm.removeItem(at: support.appendingPathComponent("update.log"))
            spawn(app, how)
            return true
        } catch {
            let reason = (error as NSError).localizedDescription
            let broken = installRoot.appendingPathComponent(appName + ".broken")
            try? fm.removeItem(at: broken)
            if fm.fileExists(atPath: app.path) { try? fm.moveItem(at: app, to: broken) }
            if fm.fileExists(atPath: previous.path) { try? fm.moveItem(at: previous, to: app) }
            writeFailure(support: support, commit: stagedCommit, reason: reason)
            if fm.fileExists(atPath: app.path) { spawn(app, how) }
            return false
        }
    }

    static func spawn(_ app: URL, _ how: Relaunch) {
        let p = Process()
        switch how {
        case .open:
            p.executableURL = URL(fileURLWithPath: "/usr/bin/open")
            p.arguments = ["-g", "-j", app.path]
        case .exec:
            guard let exe = executable(in: app) else { return }
            p.executableURL = exe
        }
        try? p.run()
    }

    static func helperMain(args: [String: String]) -> Never {
        let wait = Int32(args["wait-pid"] ?? "0") ?? 0
        let root = URL(fileURLWithPath: (args["install-root"] ?? Channel.installRoot.path) as String)
        let support = URL(fileURLWithPath: (args["support"] ?? Channel.appSupport.path) as String)
        let how: Relaunch = args["relaunch"] == "exec" ? .exec : .open
        let deadline = Date().addingTimeInterval(30)
        while wait > 0 && kill(wait, 0) == 0 && Date() < deadline {
            usleep(20_000)
        }
        let t0 = Date()
        let ok = apply(installRoot: root, support: support, relaunch: how)
        let dt = Date().timeIntervalSince(t0)
        let report: [String: Any] = ["ok": ok, "restart_s": dt]
        if let data = try? JSONSerialization.data(withJSONObject: report) {
            try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
            try? data.write(to: support.appendingPathComponent("restart.json"))
        }
        exit(ok ? 0 : 1)
    }

    /// Double fork and setsid, then exec this binary as the helper. The caller keeps running.
    @discardableResult
    static func detach(waitPid: pid_t, installRoot: URL, support: URL, relaunch how: Relaunch) -> Bool {
        let bin = Bundle.main.executableURL?.path ?? CommandLine.arguments[0]
        let rest = [
            "--update-helper",
            "--wait-pid", "\(waitPid)",
            "--install-root", installRoot.path,
            "--support", support.path,
            "--relaunch", how == .exec ? "exec" : "open",
        ]
        let pid = libcFork()
        if pid < 0 {
            log("update helper fork failed")
            return false
        }
        if pid == 0 {
            _ = setsid()
            let pid2 = libcFork()
            if pid2 == 0 {
                let nul = open("/dev/null", O_RDWR)
                if nul >= 0 {
                    _ = dup2(nul, 0)
                    _ = dup2(nul, 1)
                    _ = dup2(nul, 2)
                    if nul > 2 { close(nul) }
                }
                var c: [UnsafeMutablePointer<CChar>?] = ([bin] + rest).map { strdup($0) }
                c.append(nil)
                c.withUnsafeMutableBufferPointer { buf in
                    _ = execv(bin, buf.baseAddress)
                }
                _exit(127)
            }
            _exit(0)
        }
        var status: Int32 = 0
        waitpid(pid, &status, 0)
        return true
    }
}

enum UpdateSelfTest {
    static func run(_ dir: String) -> Never {
        let root = URL(fileURLWithPath: (dir as NSString).standardizingPath, isDirectory: true)
        let real = Channel.installRoot.standardizedFileURL.path
        if root.path == real || root.path.hasPrefix(real + "/") {
            log("refusing: selftest will not use the real Applications folder")
            exit(2)
        }
        let fm = FileManager.default
        try? fm.removeItem(at: root)
        let apps = root.appendingPathComponent("Applications", isDirectory: true)
        let support = root.appendingPathComponent("support", isDirectory: true)
        try? fm.createDirectory(at: apps, withIntermediateDirectories: true)
        try? fm.createDirectory(at: support, withIntermediateDirectories: true)

        writeJSON(support.appendingPathComponent("staged.json"), [
            "commit": "same00000000",
            "built_at": "2026-10-01T00:00:00Z",
            "ref": "test",
            "notes": ["ignored"],
        ])
        let same = UpdateRestart.offer(support: support, running: "same00000000", dismissed: nil)

        writeJSON(support.appendingPathComponent("staged.json"), [
            "commit": "newer0000000",
            "built_at": "2026-10-01T01:02:03Z",
            "ref": "test",
            "notes": ["ship the shell", "stage the update"],
        ])
        let different = UpdateRestart.offer(support: support, running: "same00000000", dismissed: nil)

        try? writeApp(apps.appendingPathComponent(UpdateRestart.appName), marker: "old", valid: true)
        try? writeApp(support.appendingPathComponent("staged").appendingPathComponent(UpdateRestart.appName), marker: "new", valid: true)
        let stamp = root.appendingPathComponent("stamp")
        setenv("HERDR_UPDATE_STAMP", stamp.path, 1)

        let sleeper = Process()
        sleeper.executableURL = URL(fileURLWithPath: "/bin/sleep")
        sleeper.arguments = ["30"]
        try? sleeper.run()
        let detached = UpdateRestart.detach(
            waitPid: sleeper.processIdentifier,
            installRoot: apps,
            support: support,
            relaunch: .exec
        )
        kill(sleeper.processIdentifier, SIGTERM)
        var sleepStatus: Int32 = 0
        waitpid(sleeper.processIdentifier, &sleepStatus, 0)

        let restart = waitJSON(support.appendingPathComponent("restart.json"), timeout: 5)
        let installed = marker(apps.appendingPathComponent(UpdateRestart.appName))
        let previous = marker(apps.appendingPathComponent(UpdateRestart.appName + ".previous"))
        let proof = "installed=\(installed)\nprevious=\(previous)\n"
        try? Data(proof.utf8).write(to: root.appendingPathComponent("swap-proof.txt"))

        try? fm.removeItem(at: support.appendingPathComponent("restart.json"))
        try? writeApp(apps.appendingPathComponent(UpdateRestart.appName), marker: "good", valid: true)
        try? writeApp(support.appendingPathComponent("staged").appendingPathComponent(UpdateRestart.appName), marker: "bad", valid: false)
        _ = UpdateRestart.detach(waitPid: 0, installRoot: apps, support: support, relaunch: .exec)
        _ = waitJSON(support.appendingPathComponent("restart.json"), timeout: 5)
        let restored = marker(apps.appendingPathComponent(UpdateRestart.appName))
        let logText = (try? String(contentsOf: support.appendingPathComponent("update.log"), encoding: .utf8)) ?? ""

        let restartS: Double = {
            guard let v = restart?["restart_s"] else { return -1 }
            if let n = v as? NSNumber { return n.doubleValue }
            return -1
        }()
        let out: [String: Any] = [
            "detached": detached,
            "same_commit": ["update": same != nil],
            "different_commit": [
                "update": different != nil,
                "commit": different?.commit ?? "",
                "built_at": different?.builtAt ?? "",
                "notes": different?.notes ?? [],
            ],
            "swap": [
                "ok": installed == "new" && previous == "old",
                "installed": installed,
                "previous": previous,
            ],
            "rollback": [
                "restored": restored,
                "log": logText.trimmingCharacters(in: .whitespacesAndNewlines),
            ],
            "restart_s": restartS,
        ]
        let data = try! JSONSerialization.data(withJSONObject: out, options: [.sortedKeys])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
        let swapOK = installed == "new" && previous == "old"
        let rolled = restored == "good" && logText.contains("corrupt")
        exit(swapOK && rolled && restartS >= 0 && restartS < 2 && same == nil && different != nil ? 0 : 1)
    }

    private static func writeJSON(_ url: URL, _ obj: [String: Any]) {
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        if let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys]) {
            try? data.write(to: url)
        }
    }

    private static func writeApp(_ app: URL, marker text: String, valid: Bool) throws {
        let fm = FileManager.default
        if fm.fileExists(atPath: app.path) { try fm.removeItem(at: app) }
        let res = app.appendingPathComponent("Contents/Resources")
        try fm.createDirectory(at: res, withIntermediateDirectories: true)
        try Data(text.utf8).write(to: res.appendingPathComponent("marker"))
        guard valid else { return }
        let mac = app.appendingPathComponent("Contents/MacOS")
        try fm.createDirectory(at: mac, withIntermediateDirectories: true)
        let exe = mac.appendingPathComponent("relaunch")
        let script = "#!/bin/sh\nprintf '%s\\n' '\(text)' > \"${HERDR_UPDATE_STAMP:-/dev/null}\"\n"
        try Data(script.utf8).write(to: exe)
        try fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: exe.path)
    }

    private static func marker(_ app: URL) -> String {
        let url = app.appendingPathComponent("Contents/Resources/marker")
        return ((try? String(contentsOf: url, encoding: .utf8)) ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func waitJSON(_ url: URL, timeout: TimeInterval) -> [String: Any]? {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let data = try? Data(contentsOf: url),
               let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                return obj
            }
            usleep(20_000)
        }
        return nil
    }
}
