import Foundation
import Darwin

final class FileChangeMonitor {
    private let queue = DispatchQueue(label: "herdr.files", qos: .utility)
    private var sources: [DispatchSourceFileSystemObject] = []
    private var pending: DispatchWorkItem?
    private var paths: (() -> [String])?
    private var changed: (() -> Void)?

    func start(paths: @escaping () -> [String], changed: @escaping () -> Void) {
        queue.async { [weak self] in
            guard let self else { return }
            self.paths = paths
            self.changed = changed
            self.reload()
        }
    }

    func refresh(completion: @escaping () -> Void = {}) {
        queue.async { [weak self] in
            guard let self else { return }
            self.pending?.cancel()
            self.reload()
            DispatchQueue.main.async(execute: completion)
        }
    }

    private func schedule() {
        pending?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.reload() }
        pending = work
        queue.asyncAfter(deadline: .now() + .milliseconds(100), execute: work)
    }

    private func reload() {
        pending = nil
        sources.forEach { $0.cancel() }
        sources.removeAll()
        var watched = Set<String>()
        for path in paths?() ?? [] where !path.isEmpty {
            var existing = path
            while !FileManager.default.fileExists(atPath: existing), existing != "/" {
                existing = (existing as NSString).deletingLastPathComponent
            }
            watched.insert(existing)
            watched.insert((existing as NSString).deletingLastPathComponent)
        }
        for path in watched where !path.isEmpty {
            let fd = open(path, O_EVTONLY)
            guard fd >= 0 else { continue }
            let source = DispatchSource.makeFileSystemObjectSource(
                fileDescriptor: fd, eventMask: [.write, .delete, .rename, .attrib, .extend, .link, .revoke], queue: queue)
            source.setEventHandler { [weak self] in self?.schedule() }
            source.setCancelHandler { close(fd) }
            sources.append(source)
            source.resume()
        }
        changed?()
    }

    deinit {
        pending?.cancel()
        sources.forEach { $0.cancel() }
    }
}
