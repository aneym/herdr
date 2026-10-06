import AppKit
import GhosttyKit

/// One ghostty_app_t for the process. Surfaces are created per herdr terminal.
final class GhosttyRuntime {
    static var shared: GhosttyRuntime!

    let app: ghostty_app_t
    let config: ghostty_config_t

    /// `configText` is the merged config from `TerminalTheme.mergedConfig`: the shell's
    /// defaults, then Alex's Ghostty config with every keybind (and anything that would
    /// make a pane translucent) stripped, because his cmd chords relay to the herdr TUI
    /// and the native shell owns chords itself.
    static func makeConfig(_ text: String) -> ghostty_config_t {
        guard let cfg = ghostty_config_new() else { fatalError("ghostty_config_new failed") }
        let tmp = NSTemporaryDirectory() + "herdr-shell-ghostty-\(getpid()).conf"
        FileManager.default.createFile(atPath: tmp, contents: Data(text.utf8), attributes: [.posixPermissions: 0o600])
        ghostty_config_load_file(cfg, tmp)
        try? FileManager.default.removeItem(atPath: tmp)
        ghostty_config_finalize(cfg)
        let n = ghostty_config_diagnostics_count(cfg)
        for i in 0..<n {
            let d = ghostty_config_get_diagnostic(cfg, i)
            if let m = d.message { log("ghostty config: \(String(cString: m))") }
        }
        return cfg
    }

    let configText: String
    /// Configs handed to libghostty on a live mode change stay alive for the process.
    private var liveConfigs: [ghostty_config_t] = []
    private var appliedTheme: String?

    init(configText: String) {
        self.configText = configText
        let cfg = GhosttyRuntime.makeConfig(configText)
        self.config = cfg

        var rt = ghostty_runtime_config_s(
            userdata: nil,
            supports_selection_clipboard: false,
            wakeup_cb: { _ in
                DispatchQueue.main.async { GhosttyRuntime.shared?.tick() }
            },
            action_cb: { _, target, action in
                GhosttyRuntime.handleAction(target: target, action: action)
            },
            read_clipboard_cb: { userdata, _, state, mimes, mimesLen, _ in
                GhosttyRuntime.readClipboard(userdata, state: state, mimes: mimes, mimesLen: mimesLen)
            },
            confirm_read_clipboard_cb: { _, _, _, _ in },
            write_clipboard_cb: { _, _, content, len, _ in
                GhosttyRuntime.writeClipboard(content: content, len: len)
            },
            close_surface_cb: { userdata, _ in
                guard let userdata else { return }
                let view = Unmanaged<SurfaceView>.fromOpaque(userdata).takeUnretainedValue()
                DispatchQueue.main.async { view.processExited() }
            }
        )
        guard let app = ghostty_app_new(&rt, cfg) else { fatalError("ghostty_app_new failed") }
        self.app = app
    }

    func tick() { ghostty_app_tick(app) }

    /// Push the effective mode to the app and every live surface, so the theme pair
    /// (dark:X,light:Y) resolves to the same side as the chrome.
    func setColorScheme(_ mode: Mode, theme: TerminalTheme, surfaces: [SurfaceView]) {
        // libghostty only re-resolves `theme = dark:X,light:Y` on a config update, so a
        // live switch hands it the same config with the mode's single theme pinned.
        let name = mode == .dark ? theme.dark : theme.light
        if appliedTheme != name {
            appliedTheme = name
            let cfg = GhosttyRuntime.makeConfig(configText + "\ntheme = \(name)\n")
            liveConfigs.append(cfg)
            ghostty_app_update_config(app, cfg)
            for v in surfaces { if let sf = v.surface { ghostty_surface_update_config(sf, cfg) } }
        }
        let scheme = mode == .dark ? GHOSTTY_COLOR_SCHEME_DARK : GHOSTTY_COLOR_SCHEME_LIGHT
        ghostty_app_set_color_scheme(app, scheme)
        surfaces.forEach { $0.setColorScheme(mode) }
    }

    static func handleAction(target: ghostty_target_s, action: ghostty_action_s) -> Bool {
        // Ghostty's app actions (tabs, splits, title) belong to herdr, not to us;
        // returning false lets libghostty fall back. The one exception is the child
        // exit: an attach that dies (killed, crashed, taken over) would otherwise sit
        // behind Ghostty's "Press any key to close" message. Claim it and hand the
        // exit to the surface, so the registry can respawn or show who holds the pane.
        if action.tag == GHOSTTY_ACTION_SHOW_CHILD_EXITED, target.tag == GHOSTTY_TARGET_SURFACE,
           let ud = ghostty_surface_userdata(target.target.surface) {
            let view = Unmanaged<SurfaceView>.fromOpaque(ud).takeUnretainedValue()
            DispatchQueue.main.async { view.processExited() }
            return true
        }
        // Claim these so libghostty's own opener does not run (it would open a browser
        // even from an agent run). The bytes are only valid until this returns.
        if action.tag == GHOSTTY_ACTION_OPEN_URL {
            let raw = text(action.action.open_url.url, len: Int(action.action.open_url.len))
            DispatchQueue.main.async { openDetectedURL(raw) }
            return true
        }
        if action.tag == GHOSTTY_ACTION_DESKTOP_NOTIFICATION {
            let n = action.action.desktop_notification
            let title = n.title.map { String(cString: $0) } ?? ""
            let body = n.body.map { String(cString: $0) } ?? ""
            var pane = ""
            if target.tag == GHOSTTY_TARGET_SURFACE, let ud = ghostty_surface_userdata(target.target.surface) {
                pane = Unmanaged<SurfaceView>.fromOpaque(ud).takeUnretainedValue().paneId
            }
            DispatchQueue.main.async {
                let tab = Notifier.shared.tabId(forPane: pane, in: Notifier.shared.latest) ?? ""
                Notifier.shared.desktop(title: title, body: body, tab: tab)
            }
            return true
        }
        return false
    }

    /// `len` is the byte count Ghostty hands us; the pointer is not a C string.
    private static func text(_ ptr: UnsafePointer<CChar>?, len: Int) -> String {
        guard let ptr, len > 0 else { return "" }
        var n = len
        if ptr[n - 1] == 0 { n -= 1 }
        guard n > 0 else { return "" }
        return String(decoding: UnsafeRawBufferPointer(start: ptr, count: n), as: UTF8.self)
    }

    /// http/https/file/mailto only. Agent runs log and record, and never call NSWorkspace.
    static func openDetectedURL(_ raw: String) {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: trimmed), let scheme = url.scheme?.lowercased(),
              ["http", "https", "file", "mailto"].contains(scheme) else { return }
        let s = url.absoluteString
        if agentRun {
            log("open_url \(s)")
            Notifier.shared.recordOpened(s)
            return
        }
        Notifier.shared.recordOpened(s)
        NSWorkspace.shared.open(url)
    }

    static func readClipboard(
        _ userdata: UnsafeMutableRawPointer?,
        state: UnsafeMutableRawPointer?,
        mimes: UnsafePointer<UnsafePointer<CChar>?>?,
        mimesLen: Int
    ) -> ghostty_clipboard_read_result_e {
        guard let userdata else { return GHOSTTY_CLIPBOARD_READ_UNSUPPORTED }
        let view = Unmanaged<SurfaceView>.fromOpaque(userdata).takeUnretainedValue()
        guard let surface = view.surface else { return GHOSTTY_CLIPBOARD_READ_UNAVAILABLE }
        let board = ClipboardImagePaste.board
        guard let text = board.string(forType: .string) else {
            // Image-only clipboard (a screenshot, Copy Image): stage it on the server
            // that owns this surface, so an agent on Studio or ax42 gets a path on its
            // own host. Text wins when both exist, like the spawn composer's ^V.
            guard ClipboardImagePaste.userPaste, let png = ClipboardImagePaste.png(from: board) else {
                return GHOSTTY_CLIPBOARD_READ_UNAVAILABLE
            }
            let path = view.clipboardSocketPath
            DispatchQueue.global(qos: .userInitiated).async { [weak view] in
                let staged = ClipboardImagePaste.upload(png, socketPath: path)
                DispatchQueue.main.async {
                    guard let view, view.surface == surface else { return }
                    if let staged {
                        completeClipboard(surface, state: state, text: staged, mime: "text/plain")
                    } else {
                        // Never paste a client-local path: it does not exist on the agent's host.
                        ghostty_surface_deny_clipboard_request(surface, state)
                        NSSound.beep()
                        log("clipboard image upload failed socket=\(path)")
                    }
                }
            }
            return GHOSTTY_CLIPBOARD_READ_STARTED
        }
        var mime = "text/plain"
        if let mimes {
            for i in 0..<mimesLen {
                if let p = mimes[i], String(cString: p).hasPrefix("text/plain") {
                    mime = String(cString: p)
                    break
                }
            }
        }
        completeClipboard(surface, state: state, text: text, mime: mime)
        return GHOSTTY_CLIPBOARD_READ_STARTED
    }

    private static func completeClipboard(_ surface: ghostty_surface_t,
        state: UnsafeMutableRawPointer?, text: String, mime: String) {
        let bytes = Array(text.utf8) + [0]
        mime.withCString { mptr in
            bytes.withUnsafeBufferPointer { buf in
                buf.baseAddress!.withMemoryRebound(to: CChar.self, capacity: bytes.count) { dptr in
                    var content = ghostty_clipboard_content_s(mime: mptr, data: dptr, len: bytes.count - 1)
                    withUnsafePointer(to: &content) { cptr in
                        var done = ghostty_clipboard_complete_s(
                            contents: cptr, contents_len: 1,
                            available: nil, available_len: 0,
                            confirmed: true, remember: false)
                        ghostty_surface_complete_clipboard_request(surface, &done, state)
                    }
                }
            }
        }
    }

    static func writeClipboard(content: UnsafePointer<ghostty_clipboard_content_s>?, len: Int) {
        guard let content else { return }
        for i in 0..<len {
            let c = content[i]
            guard let mime = c.mime, String(cString: mime).hasPrefix("text/plain"), let data = c.data else { continue }
            let s = String(decoding: UnsafeRawBufferPointer(start: data, count: c.len), as: UTF8.self)
            DispatchQueue.main.async {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(s, forType: .string)
            }
            return
        }
    }
}

func log(_ s: String) {
    FileHandle.standardError.write(("[herdr-shell] " + s + "\n").data(using: .utf8)!)
}

func shellOpen(_ url: URL) {
    if agentRun {
        log("open ignored (--agent-run) \(url.absoluteString)")
        return
    }
    NSWorkspace.shared.open(url)
}
