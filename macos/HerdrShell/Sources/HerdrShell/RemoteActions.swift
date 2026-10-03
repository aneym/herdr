import AppKit
import Foundation

/// Actions follow the server, not the machine displaying its panes.
enum RemoteActions {
    static var environment: [String: String] = [:]
    static var ssh: [String]?
    static var remoteBin = "~/.local/bin/herdr-shell-remote"
    static var localBin = ""
    static var configError: String?
    static var last: [String: Any] = [:]
    static var approvedScopes: Set<String> = []
    static let by = "herdr-shell@" + ProcessInfo.processInfo.hostName.components(separatedBy: ".")[0].lowercased()

    static func configure(herdrBin: String) {
        let launch = ProcessInfo.processInfo.environment
        environment = launch.filter { !$0.key.hasPrefix("HERDR_") && !$0.key.hasPrefix("CLAUDE") }
        // Preserve only tool overrides, never the launching pane's identity.
        for key in ["HERDR_LANE_BIN", "HERDR_KIND_BIN", "HERDR_SOCKET_PATH"] { environment[key] = launch[key] }
        environment["HERDR_BIN_PATH"] = herdrBin
        environment["CONTROL_MODES"] = ShellPaths.modes
        environment["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:" + (environment["PATH"] ?? "")
        let home = NSHomeDirectory()
        localBin = launch["HERDR_SHELL_REMOTE_BIN"]
            ?? Bundle.main.path(forResource: "herdr-shell-remote", ofType: nil)
            ?? home + "/.local/bin/herdr-shell-remote"
        let path = launch["HERDR_SHELL_SERVER_CONFIG"] ?? home + "/.config/herdr-shell/server.json"
        guard FileManager.default.fileExists(atPath: path) else { return }
        do {
            let obj = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path)))
            guard let config = obj as? [String: Any] else { throw NSError(domain: "Invalid server config", code: 1) }
            if let value = config["ssh"] {
                guard let argv = value as? [String], !argv.isEmpty else { throw NSError(domain: "Invalid ssh config", code: 1) }
                ssh = argv
            }
            remoteBin = config["remote_bin"] as? String ?? remoteBin
        } catch { configError = "Could not read server configuration" }
    }

    static func quote(_ text: String) -> String { "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'" }

    static func run(verb: String, args: [String], done: @escaping (Bool, String) -> Void) {
        let transport = ssh == nil ? "local" : "ssh"
        DispatchQueue.global(qos: .userInitiated).async {
            var ok = false, message = ""
            do {
                if let configError { throw NSError(domain: configError, code: 1) }
                let p = Process()
                if let ssh {
                    p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
                    // Keep ~ expansion for the configured executable, while quoting all user arguments.
                    let bin = remoteBin.hasPrefix("~/") ? "$HOME/" + quote(String(remoteBin.dropFirst(2))) : quote(remoteBin)
                    p.arguments = ssh + ["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", bin] + ([verb] + args).map(quote)
                } else {
                    p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
                    let path = (localBin as NSString).expandingTildeInPath
                    guard FileManager.default.fileExists(atPath: path) else {
                        throw NSError(domain: "No herdr-shell-remote here; set ~/.config/herdr-shell/server.json to reach the herdr server", code: 1)
                    }
                    p.arguments = ["python3", path, verb] + args
                }
                var childEnvironment = environment
                if ssh == nil { childEnvironment["HERDR_SOCKET_PATH"] = ProcessInfo.processInfo.environment["HERDR_SOCKET_PATH"] }
                p.environment = childEnvironment
                let out = Pipe()
                p.standardOutput = out
                p.standardError = FileHandle.nullDevice
                let exited = DispatchSemaphore(value: 0)
                p.terminationHandler = { _ in exited.signal() }
                try p.run()
                // Drain while the process runs, so a large modes file cannot fill its pipe.
                let data = out.fileHandleForReading.readDataToEndOfFile
                let readDone = DispatchSemaphore(value: 0)
                let result = RemoteOutput()
                DispatchQueue.global().async { result.data = data(); readDone.signal() }
                if exited.wait(timeout: .now() + 20) == .timedOut {
                    p.terminate()
                    if exited.wait(timeout: .now() + 1) == .timedOut { kill(p.processIdentifier, SIGKILL) }
                    throw NSError(domain: "Remote action timed out", code: 1)
                }
                guard readDone.wait(timeout: .now() + 1) == .success,
                      let reply = try JSONSerialization.jsonObject(with: result.data) as? [String: Any] else {
                    throw NSError(domain: "Invalid remote reply", code: 1)
                }
                ok = p.terminationStatus == 0 && reply["ok"] as? Bool == true
                message = reply[ok ? "message" : "error"] as? String ?? (ok ? "Done" : "Remote action failed")
                if ok, let modes = reply["modes"] {
                    let json = try JSONSerialization.data(withJSONObject: modes, options: [.sortedKeys])
                    try json.write(to: URL(fileURLWithPath: ShellPaths.modes), options: .atomic)
                }
            } catch { ok = false; message = (error as NSError).domain }
            let success = ok, text = message
            DispatchQueue.main.async {
                last = ["verb": verb, "ok": success, "message": text, "transport": transport]
                NotificationCenter.default.post(name: .remoteActionFinished, object: nil)
                done(success, text)
            }
        }
    }

    static func slug(_ scopeURL: String?) -> String? {
        guard let scopeURL,
              let route = URLComponents(string: scopeURL)?.queryItems?.first(where: { $0.name == "route" })?.value,
              route.hasPrefix("scoping/") else { return nil }
        let slug = String(route.dropFirst(8))
        return slug.range(of: "^[a-z0-9][a-z0-9-]{0,80}$", options: .regularExpression) != nil ? slug : nil
    }

    @MainActor
    static func approve(scopeURL: String?, title: String, quote: String? = nil, done: @escaping (Bool, String) -> Void) {
        guard let slug = slug(scopeURL) else {
            last = ["verb": "approve", "ok": false, "message": "Invalid scope slug", "transport": ssh == nil ? "local" : "ssh"]
            log("approve failed: Invalid scope slug")
            if quote == nil {
                let alert = NSAlert()
                alert.messageText = "Approval failed"
                alert.informativeText = "Invalid scope slug"
                alert.runModal()
            }
            done(false, "Invalid scope slug")
            return
        }
        var words = quote
        if words == nil {
            let alert = NSAlert()
            alert.messageText = "Approve \(title)?"
            alert.addButton(withTitle: "Approve")
            alert.addButton(withTitle: "Cancel")
            let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 320, height: 24))
            field.placeholderString = "Your words, saved as the approval quote"
            field.stringValue = "approved"
            alert.accessoryView = field
            alert.window.initialFirstResponder = field
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            words = field.stringValue
        }
        // An approval without the approver's words is not one; an emptied field cancels.
        guard let said = words, !said.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        run(verb: "approve", args: [slug, "--quote=\(said)", "--by=alex"]) { ok, message in
            if !ok {
                log("approve failed: \(message)")
                if quote == nil {
                    let alert = NSAlert()
                    alert.messageText = "Approval failed"
                    alert.informativeText = message
                    alert.runModal()
                }
            }
            if ok, let scopeURL {
                approvedScopes.insert(scopeURL)
                NotificationCenter.default.post(name: .remoteActionFinished, object: nil)
                DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                    approvedScopes.remove(scopeURL)
                    NotificationCenter.default.post(name: .remoteActionFinished, object: nil)
                }
            }
            done(ok, message)
        }
    }
}

private final class RemoteOutput { var data = Data() }
extension Notification.Name { static let remoteActionFinished = Notification.Name("remoteActionFinished") }
