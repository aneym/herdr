import AppKit

/// Dev updates for an installed app (Alex, 2026-10-06: "make herdr shell dev and have it hot
/// reload so all changes immediately make it into my working client"). With
/// `defaults write com.aneyman.herdr-shell herdr.shell.devReload -bool true`, a staged
/// release applies after a few seconds without input instead of the normal idle wait, and
/// the relaunched app puts back what relaunch alone loses, then says so in a quiet toast.
enum DevReload {
    static let key = "herdr.shell.devReload"
    /// Seconds without keyboard or mouse input before a staged release applies.
    static let idleSeconds: Double = 3
    /// A marker older than this belongs to some earlier relaunch, not this one.
    static let freshSeconds: Double = 60

    static var enabled: Bool { Channel.store.bool(forKey: key) }

    /// Written by the app that is about to quit; read once by the build that replaces it.
    static var markerFile: URL { Channel.appSupport.appendingPathComponent("reload.json") }
    /// What the new app found and how long the swap took (`herdr-shell-publish` and checks read it).
    static var resultFile: URL { Channel.appSupport.appendingPathComponent("reload-result.json") }

    static func noteLeaving(to commit: String, controller: MainWindowController) {
        let obj: [String: Any] = [
            "from": Channel.commit,
            "to": commit,
            "at": Date().timeIntervalSince1970,
            "fullScreen": controller.window.styleMask.contains(.fullScreen),
            "factoryOpen": controller.state.factoryOpen,
            "selectedTab": controller.state.selectedTab ?? "",
            "focusedPane": controller.state.focusedPane ?? "",
            "wasActive": NSApp.isActive,
            "frontPid": Int(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0),
        ]
        write(obj, to: markerFile)
    }

    /// Runs once at launch, after the window is up. Does nothing unless this launch is the
    /// build a dev reload asked for.
    static func arrive(controller: MainWindowController) {
        guard let data = try? Data(contentsOf: markerFile),
              let mark = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        try? FileManager.default.removeItem(at: markerFile)
        let at = (mark["at"] as? NSNumber)?.doubleValue ?? 0
        let to = mark["to"] as? String ?? ""
        let windowS = Date().timeIntervalSince1970 - at
        guard windowS >= 0, windowS < freshSeconds, !to.isEmpty, to == Channel.commit else { return }
        if let tab = mark["selectedTab"] as? String, let pane = mark["focusedPane"] as? String,
           !tab.isEmpty, !pane.isEmpty, tab == controller.state.selectedTab {
            controller.seedFocus(tab: tab, pane: pane)
        }
        if mark["factoryOpen"] as? Bool == true, !controller.state.factoryOpen { controller.setFactory(open: true) }
        if mark["fullScreen"] as? Bool == true, !controller.window.styleMask.contains(.fullScreen) {
            controller.window.toggleFullScreen(nil)
        }
        log(String(format: "dev reload %@ -> %@, window in %.2f s", mark["from"] as? String ?? "?", to, windowS))
        // A relaunch activates the new app even with `open -g`. Hand the front back to the app
        // Alex was in, once more if launch activation arrives late.
        if mark["wasActive"] as? Bool != true, let pid = (mark["frontPid"] as? NSNumber)?.int32Value, pid > 0,
           let front = NSRunningApplication(processIdentifier: pid) {
            let back = { if NSApp.isActive { front.activate() } }
            var token: NSObjectProtocol?
            token = NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification,
                                                           object: nil, queue: .main) { _ in
                guard let observer = token else { return }
                token = nil
                NotificationCenter.default.removeObserver(observer)
                back()
            }
            back()
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                if let token { NotificationCenter.default.removeObserver(token) }
                token = nil
            }
        }
        toast("Updated to \(to.prefix(8))", in: controller.root)
        // Panes come back with the first snapshot; time that too, up to 10 s.
        let start = Date()
        var panesS: Double = -1
        func settle() {
            if panesS < 0, !controller.currentPanes.isEmpty { panesS = Date().timeIntervalSince1970 - at }
            let panes = !controller.currentPanes.isEmpty
            // Report no earlier than 1.5 s in, so `active` reflects the hand-back above.
            if (!panes && Date().timeIntervalSince(start) < 10) || Date().timeIntervalSince(start) < 1.5 {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { settle() }
                return
            }
            let result: [String: Any] = [
                "from": mark["from"] as? String ?? "",
                "to": to,
                "window_s": windowS,
                "panes_s": panesS,
                "selected_tab": controller.state.selectedTab ?? "",
                "selected_tab_before": mark["selectedTab"] as? String ?? "",
                "focused_pane": controller.state.focusedPane ?? "",
                "focused_pane_before": mark["focusedPane"] as? String ?? "",
                "sidebar_visible": controller.state.sidebarVisible,
                "mode": controller.state.mode.rawValue,
                "was_active": mark["wasActive"] as? Bool ?? false,
                "active": NSApp.isActive,
            ]
            write(result, to: resultFile)
            log("dev reload settled: \(result)")
        }
        settle()
    }

    /// A small label under the title bar that fades out by itself. Top center, so it never
    /// covers a prompt or the chat composer.
    static func toast(_ text: String, in view: NSView) {
        let label = NSTextField(labelWithString: text)
        label.font = .systemFont(ofSize: ShellType.size12, weight: .medium)
        label.textColor = .secondaryLabelColor
        label.sizeToFit()
        let pad = NSSize(width: 12, height: 5)
        let size = NSSize(width: label.frame.width + 2 * pad.width, height: label.frame.height + 2 * pad.height)
        let box = NSView(frame: NSRect(x: (view.bounds.width - size.width) / 2, y: view.bounds.height - 34 - size.height,
                                       width: size.width, height: size.height))
        box.wantsLayer = true
        box.layer?.cornerRadius = ShellRadius.curve7
        box.layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        box.layer?.borderColor = NSColor.separatorColor.cgColor
        box.layer?.borderWidth = 1
        box.autoresizingMask = [.minXMargin, .maxXMargin, .minYMargin]
        label.frame.origin = NSPoint(x: pad.width, y: pad.height)
        box.addSubview(label)
        view.addSubview(box, positioned: .above, relativeTo: nil)
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
            NSAnimationContext.runAnimationGroup({ ctx in
                ctx.duration = 0.4
                box.animator().alphaValue = 0
            }, completionHandler: { box.removeFromSuperview() })
        }
    }

    private static func write(_ obj: [String: Any], to url: URL) {
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        if let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys]) {
            try? data.write(to: url, options: .atomic)
        }
    }
}
