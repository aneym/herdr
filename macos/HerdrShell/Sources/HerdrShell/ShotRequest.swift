import AppKit

/// A picture of this app's own window on request, for agents that cannot screen-record
/// the machine it runs on (Book over ssh). Create `shot.request` in the app's support
/// directory; `shot.png` replaces it there within a few seconds. Only this window is
/// drawn, the same way the scenario hook's `shot` does, so no Screen Recording grant
/// is involved and nothing else on screen is captured.
final class ShotRequest {
    private let hook: TestHook
    private var timer: Timer?

    init(controller: MainWindowController) {
        hook = TestHook(path: "", controller: controller)
    }

    func start() {
        let t = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in self?.poll() }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    private func poll() {
        let dir = Channel.appSupport
        let request = dir.appendingPathComponent("shot.request")
        guard FileManager.default.fileExists(atPath: request.path) else { return }
        try? FileManager.default.removeItem(at: request)
        let tmp = dir.appendingPathComponent("shot.png.tmp")
        try? FileManager.default.removeItem(at: tmp)
        hook.shot(tmp.path)
        guard FileManager.default.fileExists(atPath: tmp.path) else { log("shot request: no image written"); return }
        do {
            _ = try FileManager.default.replaceItemAt(dir.appendingPathComponent("shot.png"), withItemAt: tmp)
        } catch {
            log("shot request: \(error.localizedDescription)")
        }
    }
}
