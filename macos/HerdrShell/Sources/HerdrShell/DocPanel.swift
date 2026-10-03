import AppKit
import WebKit

/// Right-hand docs column. Scope and Review are web pages; RESUME, BRIEF and DECISIONS
/// are markdown from the project folder. The web view does not take keyboard focus
/// until it is clicked, and Esc hands focus back to the pane.
final class DocPanelController: NSObject, WKNavigationDelegate {
    static let defaultWidth: CGFloat = 420
    static let minWidth: CGFloat = 320

    let view = DocPanelView()
    var onClose: (() -> Void)?
    var onWidth: ((CGFloat) -> Void)?
    private(set) var tabTitles: [String] = []
    private(set) var active: String?
    private(set) var pageTitle = ""
    private(set) var pageText = ""

    private let handle = DocWidthHandle()
    private let approve = NoFocusButton()
    private let add = NoFocusButton()
    private let close = NoFocusButton()
    private let back = NoFocusButton()
    private let forward = NoFocusButton()
    private let reload = NoFocusButton()
    private let openBrowser = NoFocusButton()
    private let empty = NSTextField(labelWithString: "")
    private let urlField = DocURLField()
    private let address = DocURLField()
    private let web: DocWebView
    private var tabButtons: [NSButton] = []
    private var docs: [DocItem] = []
    private var rowId: String?
    private var allowedHost: String?
    private var watchPath: String?
    private var watchMtime: Date?
    private var contextMtime: Date?
    private var timer: Timer?
    private weak var windowController: MainWindowController?

    override init() {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = SharedWebStore.store
        web = DocWebView(frame: .zero, configuration: config)
        super.init()
        view.wantsLayer = true
        view.onLayout = { [weak self] bounds in self?.layout(in: bounds) }
        web.navigationDelegate = self
        web.onEscape = { [weak self] in self?.returnFocus() }
        web.onCommandL = { [weak self] in
            guard let self, self.showingWeb else { return false }
            self.focusAddress()
            return true
        }
        approve.title = "Approve"
        approve.isBordered = false
        approve.font = .systemFont(ofSize: 12, weight: .medium)
        approve.target = self
        approve.action = #selector(approveScope)
        approve.isHidden = true
        add.title = "+"
        add.isBordered = false
        add.font = .systemFont(ofSize: 16, weight: .medium)
        add.target = self
        add.action = #selector(addDoc)
        close.title = "✕"
        close.isBordered = false
        close.font = .systemFont(ofSize: 12)
        close.target = self
        close.action = #selector(closed)
        empty.font = .systemFont(ofSize: 13)
        empty.textColor = .secondaryLabelColor
        urlField.isHidden = true
        urlField.placeholderString = "https://… or a file path"
        urlField.target = self
        urlField.action = #selector(commitURL)
        urlField.onEscape = { [weak self] in self?.returnFocus() }
        address.isHidden = true
        address.placeholderString = "Address"
        address.target = self
        address.action = #selector(commitAddress)
        address.onEscape = { [weak self] in self?.returnFocus() }
        for (b, title, action) in [(back, "‹", #selector(goBack)), (forward, "›", #selector(goForward)),
                                   (reload, "↻", #selector(reloadPage)), (openBrowser, "Open", #selector(openOutside))] {
            b.title = title
            b.isBordered = false
            b.font = .systemFont(ofSize: 13)
            b.target = self
            b.action = action
            b.isHidden = true
        }
        handle.onDrag = { [weak self] width in self?.onWidth?(width) }
        view.onAddressChord = { [weak self] in
            guard let self, self.showingWeb else { return false }
            self.focusAddress()
            return true
        }
        ClickRegistry.shared.set("+") { [weak self] in self?.addDoc() }
        ClickRegistry.shared.set("✕") { [weak self] in self?.closed() }
        ClickRegistry.shared.set("back") { [weak self] in self?.goBack() }
        ClickRegistry.shared.set("address") { [weak self] in self?.focusAddress() }
        ClickRegistry.shared.set("web") { [weak self] in self?.focusWeb() }
        ClickRegistry.shared.set("open_browser") { [weak self] in self?.openOutside() }
        view.addSubview(web)
        view.addSubview(empty)
        view.addSubview(urlField)
        view.addSubview(address)
        view.addSubview(back)
        view.addSubview(forward)
        view.addSubview(reload)
        view.addSubview(openBrowser)
        view.addSubview(approve)
        view.addSubview(add)
        view.addSubview(close)
        view.addSubview(handle)
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.pollFile() }
    }

    func attach(_ controller: MainWindowController) { windowController = controller }

    func apply(panel: NSColor, ink: NSColor) {
        view.layer?.backgroundColor = panel.cgColor
        empty.textColor = ink.withAlphaComponent(0.65)
        close.contentTintColor = ink
        add.contentTintColor = ink
    }

    func dump() -> [String: Any] {
        ["tabs": tabTitles, "active": active ?? NSNull(), "title": pageTitle, "text": pageText,
         "url": address.stringValue, "address_focused": address.currentEditor() != nil,
         "focused": hasFocus]
    }

    var hasFocus: Bool {
        guard let r = view.window?.firstResponder as? NSView else { return false }
        return r === view || r.isDescendant(of: view) || r === web
    }

    var hasDocs: Bool { !docs.isEmpty }
    var showingWeb: Bool { docs.first { $0.title == active }?.kind == .web }

    func focusAddress() {
        guard showingWeb else { return }
        address.isHidden = false
        view.needsLayout = true
        view.window?.makeFirstResponder(address)
    }

    func focusWeb() {
        web.allowFocus = true
        view.window?.makeFirstResponder(web)
    }

    func returnFocus() {
        web.allowFocus = false
        guard let c = windowController else { return }
        if let s = c.focusedSurface ?? c.currentPanes.first {
            c.window.makeFirstResponder(s)
        }
    }

    /// Rebuild the tab strip for the selected row. The web view reloads only when the
    /// active document changes, so a poll can call this every snapshot.
    func show(model: HerdrModel, tabId: String?) {
        let previous = rowId
        rowId = tabId
        let built = Self.items(model: model, tabId: tabId)
        let same = built.map(\.key) == docs.map(\.key) && tabId == previous
        docs = built
        tabTitles = built.map(\.title)
        let saved = tabId.flatMap { SidebarState.store.string(forKey: "herdr.shell.docLast.\($0)") }
        if active == nil || !tabTitles.contains(active ?? "") {
            active = (saved.flatMap { tabTitles.contains($0) ? $0 : nil }) ?? tabTitles.first
        }
        if tabTitles.isEmpty, let tabId {
            let lane = model.catalog.snapshot.lanes[tabId]
            let name = model.catalog.snapshot.displayName(tab: tabId, lane: lane, fallback: "")
            let label = model.snapshot?.tabs.first { $0.tab_id == tabId }?.label ?? ""
            let who = !name.isEmpty ? name : (label.isEmpty ? "this row" : label)
            empty.stringValue = "No docs for \(who)"
        }
        empty.isHidden = !tabTitles.isEmpty
        web.isHidden = tabTitles.isEmpty
        rebuildButtons()
        registerDocHooks()
        if !same { loadActive() }
        contextMtime = tabId.flatMap { ContextStore.mtime(tab: $0) }
        view.needsLayout = true
    }

    func tabButton(_ name: String) -> NSButton? {
        tabButtons.first { $0.title == name }
    }

    func layout(in bounds: NSRect) {
        let bar: CGFloat = 36
        close.frame = NSRect(x: bounds.width - 28, y: 8, width: 22, height: 22)
        add.frame = NSRect(x: bounds.width - 52, y: 6, width: 22, height: 24)
        approve.isHidden = active != "Scope" || RemoteActions.slug(rowId.flatMap { windowController?.model.catalog.snapshot.lanes[$0]?.scopeURL }) == nil
        approve.frame = NSRect(x: add.frame.minX - 70, y: 6, width: 66, height: 24)
        let tabLimit = approve.isHidden ? add.frame.minX : approve.frame.minX
        var x: CGFloat = 8
        for b in tabButtons {
            let w = min(120, max(56, b.intrinsicContentSize.width + 16))
            if x + w > tabLimit - 4 { break }
            b.frame = NSRect(x: x, y: 6, width: w, height: 24)
            x += w + 4
        }
        let chrome = showingWeb
        back.isHidden = !chrome
        forward.isHidden = !chrome
        reload.isHidden = !chrome
        address.isHidden = !chrome
        openBrowser.isHidden = !chrome
        var y = bar
        if chrome {
            back.frame = NSRect(x: 8, y: y, width: 22, height: 22)
            forward.frame = NSRect(x: 30, y: y, width: 22, height: 22)
            reload.frame = NSRect(x: 52, y: y, width: 22, height: 22)
            openBrowser.frame = NSRect(x: bounds.width - 52, y: y, width: 44, height: 22)
            address.frame = NSRect(x: 78, y: y, width: max(40, openBrowser.frame.minX - 86), height: 22)
            y += 26
        }
        urlField.frame = NSRect(x: 8, y: y, width: bounds.width - 16, height: 22)
        let top = y + (urlField.isHidden ? 0 : 26)
        web.frame = NSRect(x: 0, y: top, width: bounds.width, height: max(0, bounds.height - top))
        empty.frame = NSRect(x: 16, y: bounds.midY - 10, width: bounds.width - 32, height: 20)
        handle.frame = NSRect(x: 0, y: 0, width: 6, height: bounds.height)
        handle.current = bounds.width
    }

    // MARK: loading

    private func loadActive() {
        guard let item = docs.first(where: { $0.title == active }) ?? docs.first else {
            pageTitle = ""; pageText = ""; watchPath = nil
            return
        }
        active = item.title
        if let rowId { SidebarState.store.set(item.title, forKey: "herdr.shell.docLast.\(rowId)") }
        watchPath = item.path
        watchMtime = item.path.flatMap(Self.mtime)
        allowedHost = item.url.flatMap { URL(string: $0)?.host }
        if item.kind == .web { address.stringValue = item.url ?? "" }
        switch item.kind {
        case .web:
            if let url = item.url.flatMap(URL.init(string:)) { web.load(URLRequest(url: url)) }
        case .markdown:
            let source = (try? String(contentsOfFile: item.path ?? "", encoding: .utf8)) ?? ""
            web.loadHTMLString(MiniMarkdown.html(source, title: item.title), baseURL: item.path.flatMap { URL(fileURLWithPath: $0) })
        case .file:
            if let path = item.path { web.loadFileURL(URL(fileURLWithPath: path), allowingReadAccessTo: URL(fileURLWithPath: path).deletingLastPathComponent()) }
        }
        for b in tabButtons { b.contentTintColor = (b.title == active) ? .labelColor : .secondaryLabelColor }
    }

    private func pollFile() {
        if let rowId {
            let m = ContextStore.mtime(tab: rowId)
            if m != contextMtime, let c = windowController {
                contextMtime = m
                show(model: c.model, tabId: rowId)
                return
            }
        }
        guard let path = watchPath else { return }
        let m = Self.mtime(path)
        if m != watchMtime {
            watchMtime = m
            loadActive()
        }
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        pageText = "load failed: \(error.localizedDescription)"
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        pageText = "load failed: \(error.localizedDescription)"
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        pageTitle = webView.title ?? ""
        if let current = webView.url?.absoluteString, docs.first(where: { $0.title == active })?.kind == .web {
            address.stringValue = current
        }
        if !web.allowFocus { returnFocus() }
        webView.evaluateJavaScript("document.body ? document.body.innerText : ''") { [weak self] value, _ in
            self?.pageText = value as? String ?? ""
            if self?.pageTitle.isEmpty == true { self?.pageTitle = webView.title ?? "" }
        }
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if navigationAction.navigationType == .linkActivated, let url = navigationAction.request.url {
            let host = url.host
            if host != nil, host != allowedHost {
                openOutsideApp(url)
                decisionHandler(.cancel)
                return
            }
        }
        decisionHandler(.allow)
    }

    @objc private func approveScope() {
        guard active == "Scope", let rowId, let lane = windowController?.model.catalog.snapshot.lanes[rowId] else { return }
        RemoteActions.approve(scopeURL: lane.scopeURL, title: lane.name) { [weak self] ok, _ in
            if ok {
                self?.approve.title = "Approved"
                DispatchQueue.main.asyncAfter(deadline: .now() + 5) { self?.approve.title = "Approve" }
            }
        }
    }

    @objc private func closed() {
        if let rowId, let item = docs.first(where: { $0.title == active }), let id = item.contextId {
            ContextStore.remove(tab: rowId, id: id)
            active = nil
            if let c = windowController { show(model: c.model, tabId: rowId) }
            return
        }
        onClose?()
    }

    @objc private func goBack() { web.goBack() }
    @objc private func goForward() { web.goForward() }
    @objc private func reloadPage() { web.reload() }

    @objc private func openOutside() {
        let raw = address.stringValue.isEmpty ? (docs.first { $0.title == active }?.url ?? "") : address.stringValue
        guard let url = URL(string: raw), url.scheme == "http" || url.scheme == "https" else { return }
        openOutsideApp(url)
    }

    private func openOutsideApp(_ url: URL) {
        shellOpen(url)
    }

    @objc private func commitAddress() {
        var raw = address.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty else { return }
        if !raw.contains("://") { raw = "http://" + raw }
        guard let url = URL(string: raw) else { return }
        allowedHost = url.host
        address.stringValue = raw
        web.load(URLRequest(url: url))
    }

    @objc private func pick(_ sender: NSButton) {
        active = sender.title
        loadActive()
        returnFocus()
    }

    @objc private func addDoc() {
        urlField.isHidden.toggle()
        view.needsLayout = true
        if !urlField.isHidden { view.window?.makeFirstResponder(urlField) }
    }

    @objc private func commitURL() {
        let raw = urlField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty, let rowId else { return }
        let item: DocItem
        if raw.hasPrefix("http://") || raw.hasPrefix("https://") {
            item = DocItem(title: URL(string: raw)?.host ?? raw, kind: .web, url: raw, path: nil, contextId: nil)
            ContextStore.add(tab: rowId, kind: "url", title: item.title, ref: raw, addedBy: "you")
        } else if FileManager.default.fileExists(atPath: raw) {
            let path = URL(fileURLWithPath: raw).path
            item = DocItem(title: URL(fileURLWithPath: path).lastPathComponent, kind: path.hasSuffix(".md") ? .markdown : .file, url: nil, path: path, contextId: nil)
            ContextStore.add(tab: rowId, kind: "file", title: item.title, ref: path, addedBy: "you")
        } else { return }
        urlField.stringValue = ""
        urlField.isHidden = true
        active = item.title
        if let c = windowController { show(model: c.model, tabId: rowId) }
    }

    private func registerDocHooks() {
        ClickRegistry.shared.set("+") { [weak self] in self?.addDoc() }
        ClickRegistry.shared.set("✕") { [weak self] in self?.closed() }
        for item in docs {
            let title = item.title
            ClickRegistry.shared.set("doc_tab:\(title)") { [weak self] in
                guard let self else { return }
                self.active = title
                self.loadActive()
                self.returnFocus()
            }
        }
    }

    private func rebuildButtons() {
        for b in tabButtons { b.removeFromSuperview() }
        tabButtons = docs.map { item in
            let b = NoFocusButton()
            b.title = item.title
            b.isBordered = false
            b.font = .systemFont(ofSize: 12, weight: item.title == active ? .semibold : .regular)
            b.target = self
            b.action = #selector(pick(_:))
            view.addSubview(b)
            return b
        }
    }

    private static func mtime(_ path: String) -> Date? {
        try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as? Date
    }

    // MARK: which docs a row has

    struct DocItem: Equatable {
        var title: String
        var kind: Kind
        var url: String?
        var path: String?
        var contextId: String?
        enum Kind: String { case web, markdown, file }
        var key: String { "\(contextId ?? "")|\(title)|\(url ?? "")|\(path ?? "")" }
    }

    static func items(model: HerdrModel, tabId: String?) -> [DocItem] {
        guard let tabId else { return [] }
        let snap = model.catalog.snapshot
        let lane = snap.lanes[tabId]
        let name = snap.displayName(tab: tabId, lane: lane, fallback: model.snapshot?.tabs.first { $0.tab_id == tabId }?.label ?? "")
        var out: [DocItem] = []
        if let u = lane?.scopeURL { out.append(DocItem(title: "Scope", kind: .web, url: u, path: nil, contextId: nil)) }
        if let u = lane?.reviewURL { out.append(DocItem(title: "Review", kind: .web, url: u, path: nil, contextId: nil)) }
        if let folder = projectFolder(scopeURL: lane?.scopeURL, displayName: name) {
            for file in ["RESUME", "BRIEF", "DECISIONS"] {
                let path = (folder as NSString).appendingPathComponent("\(file).md")
                if FileManager.default.fileExists(atPath: path) {
                    out.append(DocItem(title: file, kind: .markdown, url: nil, path: path, contextId: nil))
                }
            }
        }
        out.append(contentsOf: ContextStore.load(tab: tabId).map { item in
            if item.kind == "url" {
                return DocItem(title: item.title, kind: .web, url: item.ref, path: nil, contextId: item.id)
            }
            let md = item.ref.hasSuffix(".md")
            return DocItem(title: item.title, kind: md ? .markdown : .file, url: nil, path: item.ref, contextId: item.id)
        })
        return out
    }

    /// `route=scoping/<slug>` wins. Otherwise a lane folder named with spaces turned into hyphens, if it exists.
    static func projectFolder(scopeURL: String?, displayName: String) -> String? {
        let env = ProcessInfo.processInfo.environment["HOME"] ?? ""
        let home = env.isEmpty ? NSHomeDirectory() : env
        if let scopeURL, let route = URLComponents(string: scopeURL)?.queryItems?.first(where: { $0.name == "route" })?.value {
            let bits = route.split(separator: "/").map(String.init)
            if bits.count >= 2, bits[0] == "scoping", !bits[1].isEmpty {
                return (home as NSString).appendingPathComponent(".agent-rails/scoping/\(bits[1])")
            }
        }
        let folder = displayName.replacingOccurrences(of: " ", with: "-")
        guard !folder.isEmpty else { return nil }
        let path = (home as NSString).appendingPathComponent(".agent-rails/lanes/\(folder)")
        return FileManager.default.fileExists(atPath: path) ? path : nil
    }
}

enum MiniMarkdown {
    static func html(_ source: String, title: String) -> String {
        let body = blocks(source)
        return """
        <!doctype html><html><head><meta charset="utf-8"><title>\(esc(title))</title>
        <style>
        body { font: 13px -apple-system, sans-serif; color: #1F2328; background: #F3F4F6; margin: 16px; }
        table { border-collapse: collapse; margin: 8px 0; }
        td, th { border: 1px solid #D5D8DE; padding: 4px 8px; }
        code, pre { font-family: ui-monospace, monospace; background: #E9EBEF; }
        pre { padding: 8px; }
        a { color: #0A5FC4; }
        </style></head><body>\(body)</body></html>
        """
    }

    private static func blocks(_ source: String) -> String {
        let lines = source.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var html = ""
        var i = 0
        while i < lines.count {
            let line = lines[i]
            if line.hasPrefix("```") {
                var code = ""
                i += 1
                while i < lines.count, !lines[i].hasPrefix("```") { code += esc(lines[i]) + "\n"; i += 1 }
                html += "<pre><code>\(code)</code></pre>"
                i += 1
                continue
            }
            if line.hasPrefix("|"), i + 1 < lines.count, lines[i + 1].contains("---") {
                let header = cells(line)
                i += 2
                var rows: [[String]] = []
                while i < lines.count, lines[i].hasPrefix("|") { rows.append(cells(lines[i])); i += 1 }
                html += "<table><tr>" + header.map { "<th>\(inline($0))</th>" }.joined() + "</tr>"
                html += rows.map { "<tr>" + $0.map { "<td>\(inline($0))</td>" }.joined() + "</tr>" }.joined()
                html += "</table>"
                continue
            }
            if line.hasPrefix("#") {
                let level = min(3, line.prefix(while: { $0 == "#" }).count)
                let text = String(line.dropFirst(level)).trimmingCharacters(in: .whitespaces)
                html += "<h\(level)>\(inline(text))</h\(level)>"
                i += 1
                continue
            }
            if line.hasPrefix("- ") || line.hasPrefix("* ") {
                html += "<ul>"
                while i < lines.count, lines[i].hasPrefix("- ") || lines[i].hasPrefix("* ") {
                    html += "<li>\(inline(String(lines[i].dropFirst(2))))</li>"
                    i += 1
                }
                html += "</ul>"
                continue
            }
            if line.trimmingCharacters(in: .whitespaces).isEmpty { i += 1; continue }
            html += "<p>\(inline(line))</p>"
            i += 1
        }
        return html
    }

    private static func cells(_ line: String) -> [String] {
        line.split(separator: "|", omittingEmptySubsequences: false).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
    }

    private static func inline(_ s: String) -> String {
        var out = ""
        var rest = s[...]
        while let open = rest.range(of: "[") , let mid = rest.range(of: "]("), let end = rest.range(of: ")"),
              open.lowerBound <= mid.lowerBound, mid.upperBound <= end.lowerBound {
            out += esc(String(rest[..<open.lowerBound]))
            let label = String(rest[open.upperBound..<mid.lowerBound])
            let href = String(rest[mid.upperBound..<end.lowerBound])
            out += "<a href=\"\(esc(href))\">\(esc(label))</a>"
            rest = rest[end.upperBound...]
        }
        out += esc(String(rest))
        return out.replacingOccurrences(of: "`([^`]+)`", with: "<code>$1</code>", options: .regularExpression)
    }

    private static func esc(_ s: String) -> String {
        s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
    }
}

final class DocPanelView: NSView {
    var onLayout: ((NSRect) -> Void)?
    var onAddressChord: (() -> Bool)?
    override var isFlipped: Bool { true }
    override func layout() {
        super.layout()
        onLayout?(bounds)
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.type == .keyDown, event.keyCode == 37, event.modifierFlags.contains(.command),
           onAddressChord?() == true {
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
}

final class NoFocusButton: NSButton {
    override var acceptsFirstResponder: Bool { false }
}

final class DocWidthHandle: NSView {
    var onDrag: ((CGFloat) -> Void)?
    var current: CGFloat = DocPanelController.defaultWidth
    private var origin: CGFloat = 0
    private var start: CGFloat = 0

    override func resetCursorRects() { addCursorRect(bounds, cursor: .resizeLeftRight) }
    override func mouseDown(with event: NSEvent) { origin = event.locationInWindow.x; start = current }
    override func mouseDragged(with event: NSEvent) {
        onDrag?(max(DocPanelController.minWidth, start - (event.locationInWindow.x - origin)))
    }
}

/// One persistent store for every row's web view. The id file lives in the
/// channel's Application Support directory, so a lab or preview channel does
/// not share the stable channel's cookies.
enum SharedWebStore {
    static let store: WKWebsiteDataStore = {
        let dir = channelSupportDirectory()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let stamp = dir.appendingPathComponent("webkit-store-id")
        let id: UUID
        if let text = try? String(contentsOf: stamp, encoding: .utf8),
           let existing = UUID(uuidString: text.trimmingCharacters(in: .whitespacesAndNewlines)) {
            id = existing
        } else {
            id = UUID()
            try? Data(id.uuidString.utf8).write(to: stamp, options: .atomic)
        }
        return WKWebsiteDataStore(forIdentifier: id)
    }()

    private static func channelSupportDirectory() -> URL { Channel.appSupport }
}

/// Takes focus only after a click, so opening the panel leaves the pane typing.
final class DocWebView: WKWebView {
    var allowFocus = false
    var onEscape: (() -> Void)?
    var onCommandL: (() -> Bool)?
    override var acceptsFirstResponder: Bool { allowFocus }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let mods = event.modifierFlags.intersection([.command, .shift, .option, .control])
        if event.type == .keyDown, event.keyCode == 37, mods == .command, onCommandL?() == true {
            return true
        }
        return super.performKeyEquivalent(with: event)
    }
    override func becomeFirstResponder() -> Bool {
        guard allowFocus else { return false }
        return super.becomeFirstResponder()
    }
    override func mouseDown(with event: NSEvent) {
        allowFocus = true
        window?.makeFirstResponder(self)
        super.mouseDown(with: event)
    }
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { onEscape?(); return }
        super.keyDown(with: event)
    }
}

final class DocURLField: NSTextField {
    var onEscape: (() -> Void)?
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { onEscape?(); return }
        super.keyDown(with: event)
    }
}
