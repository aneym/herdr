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
    var retry: Bool
    var reason: String
}

enum Relaunch { case open, exec }

/// Swift's overlay hides `fork`. The C function is what a detached helper needs.
private func libcFork() -> Int32 {
    guard let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "fork") else { return -1 }
    let fn = unsafeBitCast(sym, to: (@convention(c) () -> Int32).self)
    return fn()
}

private func pidIsAlive(_ pid: Int32) -> Bool {
    pid > 0 && getpgid(pid) >= 0
}

/// Installs the staged app into an install root by copying it. The shell is a
/// client; this path only copies app bundles and starts the new one.
enum UpdateRestart {
    static let appName = "Herdr Shell.app"

    static func offer(support: URL, running: String, dismissed: String?) -> UpdateOffer? {
        guard let staged = StagedRelease.load(support: support), staged.commit != running else { return nil }
        if let dismissed, dismissed == staged.commit { return nil }
        let fail = readFailure(support: support)
        if let fail, fail.commit == staged.commit, fail.bad { return nil }
        let failed = fail?.commit == staged.commit
        let intact = bundleOK(stagedApp(support))
        if failed && !intact { return nil }
        return UpdateOffer(
            commit: staged.commit,
            builtAt: staged.builtAt,
            notes: staged.notes,
            failed: failed,
            retry: failed && intact,
            reason: failed ? (fail?.reason ?? "") : ""
        )
    }

    /// `hidden` until a newer commit is staged. `retry` only while the staged copy is intact.
    static func pill(support: URL, running: String, dismissed: String?) -> String {
        guard let offer = offer(support: support, running: running, dismissed: dismissed) else { return "hidden" }
        if offer.failed && offer.retry { return "retry" }
        if offer.failed { return "failed" }
        return "update"
    }

    static func readFailure(support: URL) -> (commit: String, reason: String, bad: Bool)? {
        let url = support.appendingPathComponent("update.log")
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        guard let line = text.split(separator: "\n", omittingEmptySubsequences: true).first else { return nil }
        let raw = String(line)
        let bad = raw.hasPrefix("bad ")
        guard bad || raw.hasPrefix("failed ") else { return nil }
        let rest = raw.dropFirst(bad ? "bad ".count : "failed ".count)
        guard let space = rest.firstIndex(of: " ") else { return (String(rest), "", bad) }
        let commit = String(rest[..<space])
        let reason = String(rest[rest.index(after: space)...])
        return (commit, reason, bad)
    }

    static func writeFailure(support: URL, commit: String, reason: String, bad: Bool) {
        try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        let kind = bad ? "bad" : "failed"
        let oneLine = reason.replacingOccurrences(of: "\n", with: " ")
        let line = "\(kind) \(commit) \(oneLine)\n"
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

    static func stagedApp(_ support: URL) -> URL {
        support.appendingPathComponent("staged").appendingPathComponent(appName)
    }

    static func corruptReason(_ app: URL) -> String? {
        if executable(in: app) == nil || !signatureOK(app) { return "staged app is corrupt" }
        return nil
    }

    static func bundleOK(_ app: URL) -> Bool {
        corruptReason(app) == nil
    }

    static func signatureOK(_ app: URL) -> Bool {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/codesign")
        p.arguments = ["--verify", app.path]
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        do { try p.run() } catch { return false }
        p.waitUntilExit()
        return p.terminationStatus == 0
    }

    static func copyBundle(_ src: URL, _ dst: URL) throws {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        p.arguments = [src.path, dst.path]
        let err = Pipe()
        p.standardError = err
        p.standardOutput = FileHandle.nullDevice
        try p.run()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else {
            let text = String(data: err.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            let detail = text.trimmingCharacters(in: .whitespacesAndNewlines)
            throw NSError(
                domain: "ShellUpdate",
                code: 3,
                userInfo: [NSLocalizedDescriptionKey: detail.isEmpty ? "ditto failed" : detail]
            )
        }
    }

    @discardableResult
    static func apply(installRoot: URL, support: URL, relaunch how: Relaunch) -> Bool {
        let fm = FileManager.default
        let app = installRoot.appendingPathComponent(appName)
        let previous = installRoot.appendingPathComponent(appName + ".previous")
        let incoming = installRoot.appendingPathComponent(appName + ".incoming")
        let staged = stagedApp(support)
        let stagedCommit = StagedRelease.load(support: support)?.commit ?? ""
        if let why = corruptReason(staged) {
            writeFailure(support: support, commit: stagedCommit, reason: why, bad: true)
            if fm.fileExists(atPath: app.path) { spawn(app, how) }
            return false
        }
        var replaced = false
        do {
            if fm.fileExists(atPath: previous.path) { try fm.removeItem(at: previous) }
            if fm.fileExists(atPath: incoming.path) { try fm.removeItem(at: incoming) }
            try copyBundle(staged, incoming)
            if corruptReason(incoming) != nil {
                try? fm.removeItem(at: incoming)
                throw NSError(domain: "ShellUpdate", code: 2, userInfo: [NSLocalizedDescriptionKey: "staged app is corrupt"])
            }
            if fm.fileExists(atPath: app.path) {
                try fm.moveItem(at: app, to: previous)
                replaced = true
            }
            try fm.moveItem(at: incoming, to: app)
            if corruptReason(app) != nil {
                throw NSError(domain: "ShellUpdate", code: 2, userInfo: [NSLocalizedDescriptionKey: "staged app is corrupt"])
            }
            try fm.removeItem(at: staged)
            try? fm.removeItem(at: support.appendingPathComponent("update.log"))
            spawn(app, how)
            return true
        } catch {
            let reason = (error as NSError).localizedDescription.replacingOccurrences(of: "\n", with: " ")
            if replaced || !fm.fileExists(atPath: app.path) {
                restore(app: app, previous: previous, incoming: incoming, installRoot: installRoot)
            } else {
                try? fm.removeItem(at: incoming)
            }
            writeFailure(support: support, commit: stagedCommit, reason: reason, bad: corruptReason(staged) != nil)
            if fm.fileExists(atPath: app.path) { spawn(app, how) }
            return false
        }
    }

    static func restore(app: URL, previous: URL, incoming: URL, installRoot: URL) {
        let fm = FileManager.default
        let broken = installRoot.appendingPathComponent(appName + ".broken")
        try? fm.removeItem(at: incoming)
        if fm.fileExists(atPath: app.path) {
            try? fm.removeItem(at: broken)
            try? fm.moveItem(at: app, to: broken)
        }
        if fm.fileExists(atPath: previous.path) {
            try? fm.moveItem(at: previous, to: app)
        }
    }

    /// Production relaunch argv. `SHELL_OPEN_BIN` replaces the executable for a selftest.
    static func openArgv(for app: URL) -> [String] {
        let bin = ProcessInfo.processInfo.environment["SHELL_OPEN_BIN"] ?? "/usr/bin/open"
        return [bin, app.path]
    }

    static func spawn(_ app: URL, _ how: Relaunch) {
        switch how {
        case .open:
            let argv = openArgv(for: app)
            if let sink = ProcessInfo.processInfo.environment["SHELL_OPEN_SINK"], !sink.isEmpty,
               let data = try? JSONSerialization.data(withJSONObject: argv) {
                try? data.write(to: URL(fileURLWithPath: sink))
            }
            if ProcessInfo.processInfo.environment["SHELL_OPEN_DRY"] == "1" { return }
            let p = Process()
            p.executableURL = URL(fileURLWithPath: argv[0])
            p.arguments = Array(argv.dropFirst())
            try? p.run()
        case .exec:
            guard let exe = executable(in: app) else { return }
            let p = Process()
            p.executableURL = exe
            try? p.run()
        }
    }

    static func helperMain(args: [String: String]) -> Never {
        let wait = Int32(args["wait-pid"] ?? "0") ?? 0
        let root = URL(fileURLWithPath: (args["install-root"] ?? Channel.installRoot.path) as String)
        let support = URL(fileURLWithPath: (args["support"] ?? Channel.appSupport.path) as String)
        let how: Relaunch = args["relaunch"] == "exec" ? .exec : .open
        let deadline = Date().addingTimeInterval(30)
        while wait > 0 && pidIsAlive(wait) && Date() < deadline {
            usleep(20_000)
        }
        let t0 = Date()
        let ok = apply(installRoot: root, support: support, relaunch: how)
        let dt = Date().timeIntervalSince(t0)
        let report: [String: Any] = ["ok": ok, "restart_swap_s": dt]
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
        do {
            try body(dir)
        } catch {
            log("selftest \(error)")
            exit(1)
        }
    }

    private static func body(_ dir: String) throws -> Never {
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
        try fm.createDirectory(at: apps, withIntermediateDirectories: true)
        try fm.createDirectory(at: support, withIntermediateDirectories: true)
        let running = "same00000000"

        writeJSON(support.appendingPathComponent("staged.json"), [
            "commit": running,
            "built_at": "2026-10-01T00:00:00Z",
            "ref": "test",
            "notes": ["ignored"],
        ])
        let same = UpdateRestart.offer(support: support, running: running, dismissed: nil)

        writeJSON(support.appendingPathComponent("staged.json"), [
            "commit": "newer0000000",
            "built_at": "2026-10-01T01:02:03Z",
            "ref": "test",
            "notes": ["ship the shell", "stage the update"],
        ])
        let different = UpdateRestart.offer(support: support, running: running, dismissed: nil)

        let installedApp = apps.appendingPathComponent(UpdateRestart.appName)
        let stagedBundle = UpdateRestart.stagedApp(support)
        try writeApp(installedApp, marker: "old", valid: true)
        try writeApp(stagedBundle, marker: "new", valid: true)
        let sink = root.appendingPathComponent("open-argv.json")
        try Data(installedApp.path.utf8).write(to: root.appendingPathComponent("open-expected.txt"))
        setenv("SHELL_OPEN_BIN", "/usr/bin/open", 1)
        setenv("SHELL_OPEN_SINK", sink.path, 1)
        setenv("SHELL_OPEN_DRY", "1", 1)

        let sleeper = Process()
        sleeper.executableURL = URL(fileURLWithPath: "/bin/sleep")
        sleeper.arguments = ["30"]
        try sleeper.run()
        let detached = UpdateRestart.detach(
            waitPid: sleeper.processIdentifier,
            installRoot: apps,
            support: support,
            relaunch: .open
        )
        sleeper.terminate()
        sleeper.waitUntilExit()

        let restart = waitJSON(support.appendingPathComponent("restart.json"), timeout: 5)
        let installed = marker(installedApp)
        let previous = marker(apps.appendingPathComponent(UpdateRestart.appName + ".previous"))
        let proof = "installed=\(installed)\nprevious=\(previous)\n"
        try Data(proof.utf8).write(to: root.appendingPathComponent("swap-proof.txt"))
        let openArgv = (try? JSONSerialization.jsonObject(with: Data(contentsOf: sink))) as? [String] ?? []
        unsetenv("SHELL_OPEN_DRY")
        unsetenv("SHELL_OPEN_SINK")
        unsetenv("SHELL_OPEN_BIN")

        try writeApp(installedApp, marker: "good", valid: true)
        try writeApp(stagedBundle, marker: "bad", valid: false)
        let corruptOK = UpdateRestart.apply(installRoot: apps, support: support, relaunch: .exec)
        let restored = marker(installedApp)
        let logText = (try? String(contentsOf: support.appendingPathComponent("update.log"), encoding: .utf8)) ?? ""
        let corruptPill = UpdateRestart.pill(support: support, running: running, dismissed: nil)
        let corruptOffer = UpdateRestart.offer(support: support, running: running, dismissed: nil)
        let stagedRemains = fm.fileExists(atPath: stagedBundle.path)
        writeJSON(support.appendingPathComponent("staged.json"), [
            "commit": "fresh1111111",
            "built_at": "2026-10-01T02:00:00Z",
            "ref": "test",
            "notes": ["a newer commit"],
        ])
        let newerPill = UpdateRestart.pill(support: support, running: running, dismissed: nil)
        let corruptProof: [String: Any] = [
            "restored": restored,
            "log": logText.trimmingCharacters(in: .whitespacesAndNewlines),
            "retry": corruptOffer?.retry ?? false,
            "pill": corruptPill,
            "staged_remains": stagedRemains,
            "newer_pill": newerPill,
            "apply": corruptOK,
        ]
        writeJSON(root.appendingPathComponent("corrupt-proof.json"), corruptProof)

        try writeApp(installedApp, marker: "kept", valid: true)
        try writeApp(stagedBundle, marker: "next", valid: true)
        writeJSON(support.appendingPathComponent("staged.json"), [
            "commit": "retry1111111",
            "built_at": "2026-10-01T03:00:00Z",
            "ref": "test",
            "notes": ["retry me"],
        ])
        let mode = posixMode(apps)
        try fm.setAttributes([.posixPermissions: NSNumber(value: UInt16(0o555))], ofItemAtPath: apps.path)
        let firstOK = UpdateRestart.apply(installRoot: apps, support: support, relaunch: .exec)
        try fm.setAttributes([.posixPermissions: NSNumber(value: mode)], ofItemAtPath: apps.path)
        let afterFail = marker(installedApp)
        let stagedKept = marker(stagedBundle)
        let retryOffer = UpdateRestart.offer(support: support, running: running, dismissed: nil)
        let retryPill = UpdateRestart.pill(support: support, running: running, dismissed: nil)
        let secondOK = UpdateRestart.apply(installRoot: apps, support: support, relaunch: .exec)
        let afterRetry = marker(installedApp)
        let stagedGone = !fm.fileExists(atPath: stagedBundle.path)
        let retryProof: [String: Any] = [
            "first_ok": firstOK,
            "restored": afterFail,
            "staged": stagedKept,
            "retry": retryOffer?.retry ?? false,
            "pill": retryPill,
            "second_ok": secondOK,
            "installed": afterRetry,
            "staged_gone": stagedGone,
        ]
        writeJSON(root.appendingPathComponent("retry-proof.json"), retryProof)

        let restartS: Double = {
            guard let v = restart?["restart_swap_s"] else { return -1 }
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
            "open_argv": openArgv,
            "rollback": corruptProof,
            "retry": retryProof,
            "restart_swap_s": restartS,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: out, options: [.sortedKeys]) else { exit(1) }
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
        let swapOK = installed == "new" && previous == "old"
        let openOK = openArgv == ["/usr/bin/open", installedApp.path]
        let rolled = !corruptOK && restored == "good" && logText.contains("corrupt") && logText.hasPrefix("bad ")
            && corruptPill == "hidden" && corruptOffer == nil && stagedRemains && newerPill == "update"
        let retried = !firstOK && afterFail == "kept" && stagedKept == "next" && retryOffer?.retry == true
            && retryPill == "retry" && secondOK && afterRetry == "next" && stagedGone
        exit(swapOK && openOK && rolled && retried && restartS >= 0 && restartS < 2 && same == nil && different != nil && detached ? 0 : 1)
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
        let info: [String: Any] = [
            "CFBundlePackageType": "APPL",
            "CFBundleExecutable": "relaunch",
            "CFBundleIdentifier": "com.example.shell.selftest",
            "CFBundleName": "Shell",
            "CFBundleVersion": "1",
            "CFBundleShortVersionString": "1",
        ]
        let plist = try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        try plist.write(to: app.appendingPathComponent("Contents/Info.plist"))
        let mac = app.appendingPathComponent("Contents/MacOS")
        try fm.createDirectory(at: mac, withIntermediateDirectories: true)
        try fm.copyItem(at: URL(fileURLWithPath: "/usr/bin/true"), to: mac.appendingPathComponent("relaunch"))
        let sign = Process()
        sign.executableURL = URL(fileURLWithPath: "/usr/bin/codesign")
        sign.arguments = ["--sign", "-", "--force", app.path]
        let err = Pipe()
        sign.standardError = err
        sign.standardOutput = FileHandle.nullDevice
        try sign.run()
        sign.waitUntilExit()
        guard sign.terminationStatus == 0 else {
            let text = String(data: err.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            throw NSError(domain: "ShellUpdate", code: 4, userInfo: [NSLocalizedDescriptionKey: text])
        }
    }

    private static func posixMode(_ url: URL) -> UInt16 {
        let raw = try? FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions]
        return (raw as? NSNumber)?.uint16Value ?? 0o755
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
