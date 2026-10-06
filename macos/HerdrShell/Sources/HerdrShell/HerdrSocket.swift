import Foundation

/// Blocking Unix-domain socket helpers for herdr's line-delimited JSON API.
enum HerdrSocket {
    static func connect(_ path: String) -> Int32? {
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return nil }
        var one: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        guard bytes.count < MemoryLayout.size(ofValue: addr.sun_path) else { close(fd); return nil }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            for (i, b) in bytes.enumerated() { raw[i] = b }
            raw[bytes.count] = 0
        }
        let len = socklen_t(MemoryLayout<sockaddr_un>.size)
        let rc = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, len) }
        }
        if rc != 0 { close(fd); return nil }
        return fd
    }

    @discardableResult
    static func writeAll(_ fd: Int32, _ data: Data) -> Bool {
        var off = 0
        return data.withUnsafeBytes { (p: UnsafeRawBufferPointer) -> Bool in
            while off < data.count {
                let n = write(fd, p.baseAddress! + off, data.count - off)
                if n <= 0 { return false }
                off += n
            }
            return true
        }
    }

    /// One request, one response line, on a fresh connection. `timeout` bounds the wait.
    static func request(_ path: String, _ json: String, timeout: TimeInterval = 5) -> Data? {
        guard let fd = connect(path) else { return nil }
        defer { close(fd) }
        guard writeAll(fd, Data((json + "\n").utf8)) else { return nil }
        var buf = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        let deadline = Date().addingTimeInterval(timeout)
        while true {
            if let nl = buf.firstIndex(of: 0x0A) { return buf.prefix(upTo: nl) }
            let remaining = deadline.timeIntervalSinceNow
            if remaining <= 0 { return nil }
            var pfd = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
            let pr = poll(&pfd, 1, Int32(min(remaining, 1) * 1000))
            if pr < 0 { if errno == EINTR { continue }; return nil }
            if pr == 0 { continue }
            let n = read(fd, &chunk, chunk.count)
            if n <= 0 { return nil }
            buf.append(chunk, count: n)
        }
    }
}

