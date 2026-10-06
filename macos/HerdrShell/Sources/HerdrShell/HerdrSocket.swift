import Foundation

/// Blocking Unix-domain socket helpers for herdr's line-delimited JSON API.
enum HerdrSocket {
    /// Blocking connect. With a `deadline` the socket is left non-blocking and the
    /// connect waits only until then, so a stalled forwarded peer cannot hang the caller.
    static func connect(_ path: String, deadline: Date? = nil) -> Int32? {
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
        if deadline != nil {
            let flags = fcntl(fd, F_GETFL)
            guard flags >= 0, fcntl(fd, F_SETFL, flags | O_NONBLOCK) == 0 else { close(fd); return nil }
        }
        let len = socklen_t(MemoryLayout<sockaddr_un>.size)
        let rc = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, len) }
        }
        if rc == 0 { return fd }
        guard let deadline, errno == EINPROGRESS || errno == EAGAIN || errno == EINTR,
              waitFor(fd, Int16(POLLOUT), deadline) else { close(fd); return nil }
        var err: Int32 = 0
        var errLen = socklen_t(MemoryLayout<Int32>.size)
        guard getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &errLen) == 0, err == 0 else { close(fd); return nil }
        return fd
    }

    /// Polls until `events` is ready on `fd` or `deadline` passes.
    private static func waitFor(_ fd: Int32, _ events: Int16, _ deadline: Date) -> Bool {
        while true {
            let remaining = deadline.timeIntervalSinceNow
            if remaining <= 0 { return false }
            var pfd = pollfd(fd: fd, events: events, revents: 0)
            let pr = poll(&pfd, 1, Int32(max(1, min(remaining, 1) * 1000)))
            if pr < 0 { if errno == EINTR { continue }; return false }
            if pr == 0 { continue }
            return pfd.revents & (events | Int16(POLLHUP) | Int16(POLLERR)) != 0
        }
    }

    /// Writes every byte. With a `deadline` (non-blocking fd from `connect(_:deadline:)`)
    /// a peer that stops draining fails the write instead of blocking past it.
    @discardableResult
    static func writeAll(_ fd: Int32, _ data: Data, deadline: Date? = nil) -> Bool {
        var off = 0
        return data.withUnsafeBytes { (p: UnsafeRawBufferPointer) -> Bool in
            guard let base = p.baseAddress else { return data.isEmpty }
            while off < data.count {
                let n = write(fd, base + off, data.count - off)
                if n > 0 { off += n; continue }
                if n < 0, let deadline, errno == EAGAIN || errno == EINTR {
                    if errno == EINTR { continue }
                    guard waitFor(fd, Int16(POLLOUT), deadline) else { return false }
                    continue
                }
                return false
            }
            return true
        }
    }

    /// One request, one response line, on a fresh connection. `timeout` bounds the
    /// whole exchange: connect, request write and response read.
    static func request(_ path: String, _ json: String, timeout: TimeInterval = 5) -> Data? {
        let deadline = Date().addingTimeInterval(timeout)
        guard let fd = connect(path, deadline: deadline) else { return nil }
        defer { close(fd) }
        guard writeAll(fd, Data((json + "\n").utf8), deadline: deadline) else { return nil }
        var buf = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            if let nl = buf.firstIndex(of: 0x0A) { return buf.prefix(upTo: nl) }
            guard waitFor(fd, Int16(POLLIN), deadline) else { return nil }
            let n = read(fd, &chunk, chunk.count)
            if n < 0, errno == EAGAIN || errno == EINTR { continue }
            if n <= 0 { return nil }
            buf.append(chunk, count: n)
        }
    }
}
