import AppKit

/// Quiet update affordance in the title bar. The design pass owns the look.
final class UpdateController: NSObject {
    private weak var windowController: MainWindowController?
    private let accessory = NSTitlebarAccessoryViewController()
    private let box = NSView(frame: NSRect(x: 0, y: 0, width: 96, height: 22))
    private let pill = NSButton(title: "Update", target: nil, action: nil)
    private let popover = NSPopover()
    private var source: DispatchSourceFileSystemObject?
    private var timer: Timer?
    private var current: UpdateOffer?

    init(controller: MainWindowController) {
        self.windowController = controller
        super.init()
        pill.target = self
        pill.action = #selector(togglePopover)
        pill.bezelStyle = .recessed
        pill.font = .systemFont(ofSize: 12, weight: .medium)
        pill.setButtonType(.momentaryPushIn)
        pill.frame = NSRect(x: 0, y: 1, width: 76, height: 20)
        box.addSubview(pill)
        accessory.layoutAttribute = .right
        accessory.view = box
        controller.window.addTitlebarAccessoryViewController(accessory)
        pill.isHidden = true
        popover.behavior = .transient
    }

    var shown: Bool { !pill.isHidden }

    func start() {
        let dir = Channel.appSupport
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let fd = open(dir.path, O_EVTONLY)
        if fd >= 0 {
            let src = DispatchSource.makeFileSystemObjectSource(
                fileDescriptor: fd,
                eventMask: [.write, .rename, .delete, .extend],
                queue: .main
            )
            src.setEventHandler { [weak self] in self?.reload(show: false) }
            src.setCancelHandler { close(fd) }
            src.resume()
            source = src
        }
        let t = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            self?.reload(show: false)
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
        reload(show: false)
    }

    func checkNow() { reload(show: true) }

    private func reload(show: Bool) {
        let dismissed = show ? nil : Channel.store.string(forKey: Channel.dismissedKey)
        current = UpdateRestart.offer(support: Channel.appSupport, running: Channel.commit, dismissed: dismissed)
        applyPill()
        if show, current != nil { openPopover() }
    }

    private func applyPill() {
        guard let current else {
            pill.isHidden = true
            return
        }
        pill.isHidden = false
        if current.failed {
            pill.title = current.retry ? "Update failed — Retry" : "Update failed"
        } else {
            pill.title = "Update"
        }
        pill.sizeToFit()
        let width = max(pill.frame.width + 18, 72)
        pill.frame = NSRect(x: 0, y: 1, width: width, height: 20)
        box.frame = NSRect(x: 0, y: 0, width: width + 8, height: 22)
        windowController?.root.titleReserve = pill.isHidden ? 0 : 28
    }

    @objc private func togglePopover() {
        if popover.isShown { popover.performClose(nil) } else { openPopover() }
    }

    private func openPopover() {
        guard let current else { return }
        popover.contentViewController = UpdatePopoverController(offer: current, owner: self)
        popover.show(relativeTo: pill.bounds, of: pill, preferredEdge: .minY)
    }

    func restartNow() {
        guard let windowController else { return }
        windowController.saveForRelaunch()
        popover.performClose(nil)
        if UpdateRestart.detach(waitPid: getpid(), installRoot: Channel.installRoot, support: Channel.appSupport, relaunch: .open) {
            NSApp.terminate(nil)
        }
    }

    func later() {
        if let commit = current?.commit {
            Channel.store.set(commit, forKey: Channel.dismissedKey)
            Channel.store.synchronize()
        }
        current = nil
        pill.isHidden = true
        popover.performClose(nil)
    }
}

final class UpdatePopoverController: NSViewController {
    let offer: UpdateOffer
    weak var owner: UpdateController?

    init(offer: UpdateOffer, owner: UpdateController) {
        self.offer = offer
        self.owner = owner
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let width: CGFloat = 280
        let pad: CGFloat = 16
        // The newest few changes; a long gap since the last update collapses to a count.
        let shown = Array(offer.notes.prefix(4))
        var lines = shown.map { "• \($0)" }
        if offer.notes.count > shown.count { lines.append("and \(offer.notes.count - shown.count) more") }
        let notes = lines.isEmpty ? "No notes." : lines.joined(separator: "\n")
        let meta = [String(offer.commit.prefix(8)), Self.when(offer.builtAt)].filter { !$0.isEmpty }.joined(separator: " · ")

        let title = NSTextField(labelWithString: "What's new")
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        let text = NSTextField(wrappingLabelWithString: notes)
        text.font = .systemFont(ofSize: 12)
        text.textColor = .secondaryLabelColor
        text.preferredMaxLayoutWidth = width - 2 * pad
        let foot = NSTextField(labelWithString: offer.reason.isEmpty ? meta : offer.reason)
        foot.font = .systemFont(ofSize: 11)
        foot.textColor = .tertiaryLabelColor
        foot.lineBreakMode = .byTruncatingTail

        let primary = offer.failed && offer.retry ? "Retry" : "Restart to update"
        let go = NSButton(title: primary, target: self, action: #selector(restart))
        go.bezelStyle = .rounded
        go.keyEquivalent = "\r"
        let later = NSButton(title: "Later", target: self, action: #selector(later))
        later.bezelStyle = .rounded

        let textH = ceil(text.sizeThatFits(NSSize(width: width - 2 * pad, height: 1000)).height)
        let height = 14 + 18 + 6 + textH + 8 + 14 + 12 + 28 + 10
        let view = NSView(frame: NSRect(x: 0, y: 0, width: width, height: height))
        var y = height - 14 - 18
        title.frame = NSRect(x: pad, y: y, width: width - 2 * pad, height: 18)
        y -= 6 + textH
        text.frame = NSRect(x: pad, y: y, width: width - 2 * pad, height: textH)
        y -= 8 + 14
        foot.frame = NSRect(x: pad, y: y, width: width - 2 * pad, height: 14)
        later.frame = NSRect(x: pad - 6, y: 10, width: 80, height: 28)
        go.frame = NSRect(x: width - pad + 6 - 150, y: 10, width: 150, height: 28)

        view.addSubview(title)
        view.addSubview(text)
        view.addSubview(foot)
        if !(offer.failed && !offer.retry) { view.addSubview(go) }
        view.addSubview(later)
        self.view = view
    }

    /// "Oct 2, 7:08 PM" in local time from the bundle's UTC stamp.
    private static func when(_ iso: String) -> String {
        guard let d = ISO8601DateFormatter().date(from: iso) else { return iso }
        let f = DateFormatter()
        f.dateFormat = "MMM d, h:mm a"
        return f.string(from: d)
    }

    @objc private func restart() { owner?.restartNow() }
    @objc private func later() { owner?.later() }
}
