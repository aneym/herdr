import AppKit

enum ClipboardImagePaste {
    static let maxBytes = 16 * 1024 * 1024
    /// The pasteboard a terminal paste reads. Agent-run checks point it at a private
    /// named board so they never touch the user's clipboard.
    static var board: NSPasteboard = .general
    /// True only inside a user paste (Edit > Paste, Cmd-V). A program's OSC 52 read
    /// goes through the same Ghostty callback and must never upload an image.
    static var userPaste = false

    static func png(from pasteboard: NSPasteboard) -> Data? {
        if let data = pasteboard.data(forType: .png) { return data }
        guard let data = pasteboard.data(forType: .tiff),
              let bitmap = NSBitmapImageRep(data: data) else { return nil }
        return bitmap.representation(using: .png, properties: [:])
    }

    /// Blocking; callers run this off the main thread and retain the paste target.
    static func upload(_ png: Data, socketPath: String) -> String? {
        guard !socketPath.isEmpty, png.count <= maxBytes,
              png.starts(with: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) else { return nil }
        if servedByLocalHerdr(socketPath), let path = stageLocally(png) { return path }
        let body: [String: Any] = [
            "id": "shell:clipboard-image", "method": "clipboard.image.write",
            "params": ["extension": "png", "data_base64": png.base64EncodedString()]
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let json = String(data: data, encoding: .utf8),
              let reply = HerdrSocket.request(socketPath, json, timeout: 30),
              let obj = try? JSONSerialization.jsonObject(with: reply) as? [String: Any],
              obj["error"] == nil,
              let result = obj["result"] as? [String: Any],
              let text = result["paste_text"] as? String, !text.isEmpty else { return nil }
        return text
    }

    static func servedByLocalHerdr(_ socketPath: String) -> Bool {
        guard let fd = HerdrSocket.connect(socketPath, deadline: Date().addingTimeInterval(2)) else { return false }
        defer { close(fd) }
        var pid: pid_t = 0
        var length = socklen_t(MemoryLayout<pid_t>.size)
        guard getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &pid, &length) == 0, pid > 0 else { return false }
        var path = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
        guard proc_pidpath(pid, &path, UInt32(path.count)) > 0 else { return false }
        return URL(fileURLWithPath: String(cString: path)).lastPathComponent == "herdr"
    }

    private static func stageLocally(_ png: Data) -> String? {
        let runtime = ProcessInfo.processInfo.environment["XDG_RUNTIME_DIR"] ?? ""
        let root = runtime.hasPrefix("/") ? runtime : "/var/tmp"
        let dir = root + "/herdr-clipboard-images-\(geteuid())"
        guard mkdir(dir, 0o700) == 0 || errno == EEXIST else { return nil }
        let directory = open(dir, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard directory >= 0 else { return nil }
        defer { close(directory) }
        var metadata = stat()
        guard fstat(directory, &metadata) == 0, metadata.st_uid == geteuid(),
              metadata.st_mode & 0o777 == 0o700 else { return nil }
        let name = "shell-clipboard-\(UUID().uuidString).png"
        let file = openat(directory, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard file >= 0 else { return nil }
        var complete = false
        defer {
            close(file)
            if !complete { unlinkat(directory, name, 0) }
        }
        complete = png.withUnsafeBytes { bytes in
            guard let base = bytes.baseAddress else { return false }
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(file, base.advanced(by: offset), bytes.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { return false }
                offset += count
            }
            return true
        }
        return complete ? dir + "/" + name : nil
    }
}
