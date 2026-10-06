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
    private(set) var activeItem: String?
    private var activeKey: String?
    private var fronts: [String: String] = [:]
    private var activeKeys: [String: String] = [:]
    private var fileMtime: UInt64?
    private var fileData: Data?
    private var readInFlight = false
    private var readGeneration = 0
    private var transient: [String: [DeskItem]] = [:]
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
    var showingWeb: Bool { docs.first { $0.key == activeKey }?.kind == .web }

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
        var built = Self.items(model: model, tabId: tabId)
        if let tabId { built += (transient[tabId] ?? []).map(Self.docItem) }
        let same = built.map(\.key) == docs.map(\.key) && tabId == previous
        docs = built
        tabTitles = built.map(\.title)
        let desk = tabId.flatMap { model.source(for: $0) }?.tabs.first { $0.tab_id == tabId }?.desk ?? .empty
        let oldKey = tabId.flatMap { activeKeys[$0] }
        let selectedDesk = deskFront(previousFront: tabId.flatMap { fronts[$0] }, current: desk,
                                     active: oldKey.flatMap { key in built.first { $0.key == key }?.deskId })
        let frontChanged = tabId.flatMap { fronts[$0] } != desk.front
        if let selectedDesk, frontChanged || oldKey == nil || !built.contains(where: { $0.key == oldKey }) {
            activeKey = built.first { $0.deskId == selectedDesk }?.key
        } else {
            activeKey = oldKey.flatMap { key in built.contains { $0.key == key } ? key : nil } ?? built.first?.key
        }
        if let tabId { fronts[tabId] = desk.front }
        active = built.first { $0.key == activeKey }?.title
        activeItem = built.first { $0.key == activeKey }?.deskId
        if tabTitles.isEmpty, let tabId {
            let lane = model.catalog.snapshot.lanes[tabId]
            let name = model.catalog.snapshot.displayName(tab: tabId, lane: lane, fallback: "")
            let label = model.source(for: tabId)?.tabs.first { $0.tab_id == tabId }?.label ?? ""
            let who = !name.isEmpty ? name : (label.isEmpty ? "this row" : label)
            empty.stringValue = "No docs for \(who)"
        }
        empty.isHidden = !tabTitles.isEmpty
        web.isHidden = tabTitles.isEmpty
        rebuildButtons()
        registerDocHooks()
        if !same || oldKey != activeKey { loadActive() }
        view.needsLayout = true
    }

    func tabButton(_ name: String) -> NSButton? {
        tabButtons.first { $0.title == name }
    }

    func layout(in bounds: NSRect) {
        let bar: CGFloat = 36
        close.frame = NSRect(x: bounds.width - 28, y: 8, width: 22, height: 22)
        add.frame = NSRect(x: bounds.width - 52, y: 6, width: 22, height: 24)
        approve.isHidden = activeItem != nil || active != "Scope" || RemoteActions.slug(rowId.flatMap { windowController?.model.catalog.snapshot.lanes[$0]?.scopeURL }) == nil
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
        openBrowser.isHidden = docs.isEmpty
        var y = bar
        if !chrome { openBrowser.frame = NSRect(x: bounds.width - 52, y: y, width: 44, height: 22); y += 26 }
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
        guard let item = docs.first(where: { $0.key == activeKey }) ?? docs.first else {
            readGeneration += 1
            readInFlight = false
            activeItem = nil; fileData = nil; fileMtime = nil
            pageTitle = ""; pageText = ""; watchPath = nil
            web.loadHTMLString("", baseURL: nil)
            return
        }
        readGeneration += 1
        readInFlight = false
        fileMtime = nil
        fileData = nil
        activeKey = item.key
        activeItem = item.deskId
        active = item.title
        pageTitle = ""; pageText = ""
        view.needsLayout = true
        for (index, button) in tabButtons.enumerated() {
            button.contentTintColor = docs[index].key == activeKey ? .labelColor : .secondaryLabelColor
            button.font = .systemFont(ofSize: 12, weight: docs[index].key == activeKey ? .semibold : .regular)
        }
        if let rowId { activeKeys[rowId] = item.key }
        if let rowId { SidebarState.store.set(item.title, forKey: "herdr.shell.docLast.\(rowId)") }
        watchPath = item.path
        watchMtime = item.deskId == nil ? item.path.flatMap(Self.mtime) : nil
        allowedHost = item.url.flatMap { URL(string: $0)?.host }
        if item.kind == .web { address.stringValue = item.url ?? "" }
        if item.deskId != nil, item.kind != .web {
            watchPath = nil
            web.loadHTMLString("", baseURL: nil)
            readDeskFile(item)
            return
        }
        switch item.kind {
        case .web:
            if let url = item.url.flatMap(URL.init(string:)) { web.load(URLRequest(url: url)) }
        case .markdown:
            let source = (try? String(contentsOfFile: item.path ?? "", encoding: .utf8)) ?? ""
            web.loadHTMLString(MiniMarkdown.html(source, title: item.title), baseURL: item.path.flatMap { URL(fileURLWithPath: $0) })
        case .file:
            if let path = item.path { web.loadFileURL(URL(fileURLWithPath: path), allowingReadAccessTo: URL(fileURLWithPath: path).deletingLastPathComponent()) }
        }
    }

    private func pollFile() {
        if let item = docs.first(where: { $0.key == activeKey }), item.deskId != nil, item.kind != .web {
            if !view.isHidden { readDeskFile(item) }
            return
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
        if let current = webView.url?.absoluteString, docs.first(where: { $0.key == activeKey })?.kind == .web {
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
        guard docs.first(where: { $0.key == activeKey })?.deskId == nil, active == "Scope", let rowId, let lane = windowController?.model.catalog.snapshot.lanes[rowId] else { return }
        RemoteActions.approve(scopeURL: lane.scopeURL, title: lane.name) { [weak self] ok, _ in
            if ok {
                self?.approve.title = "Approved"
                DispatchQueue.main.asyncAfter(deadline: .now() + 5) { self?.approve.title = "Approve" }
            }
        }
    }

    @objc private func closed() {
        if let rowId, let item = docs.first(where: { $0.key == activeKey }), let id = item.deskId {
            if id.hasPrefix("local-") {
                transient[rowId]?.removeAll { $0.id == id }
                activeKeys[rowId] = nil
                if let c = windowController { show(model: c.model, tabId: rowId) }
            } else { deskCommand("desk.close", item: id) }
            return
        }
        onClose?()
    }

    /// ⌘W on the column: the ✕ action, then the pane gets the keys back.
    func closeActive() {
        closed()
        returnFocus()
    }

    @objc private func goBack() { web.goBack() }
    @objc private func goForward() { web.goForward() }
    @objc private func reloadPage() { web.reload() }

    @objc private func openOutside() {
        guard let item = docs.first(where: { $0.key == activeKey }) else { return }
        if item.kind == .web {
            let raw = address.stringValue.isEmpty ? item.url ?? "" : address.stringValue
            if let url = URL(string: raw), ["http", "https"].contains(url.scheme?.lowercased() ?? "") {
                NSWorkspace.shared.open(url)
            }
        } else if let path = item.path {
            let generation = readGeneration, row = rowId, id = item.deskId
            let data = fileData
            let remote = row.map(Machines.isRemote) == true
            DispatchQueue.global(qos: .utility).async { [weak self] in
                do {
                    let url: URL
                    if !remote && FileManager.default.fileExists(atPath: path) {
                        url = URL(fileURLWithPath: path)
                    } else if let data, let id {
                        let dir = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("herdr-desk")
                        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                        url = dir.appendingPathComponent("\(id)-\(URL(fileURLWithPath: path).lastPathComponent)")
                        try data.write(to: url, options: .atomic)
                    } else { return }
                    DispatchQueue.main.async {
                        guard let self, generation == self.readGeneration, self.rowId == row,
                              self.activeKey == item.key, self.activeItem == id else { return }
                        NSWorkspace.shared.open(url)
                    }
                } catch {
                    let message = error.localizedDescription
                    DispatchQueue.main.async {
                        guard let self, generation == self.readGeneration, self.rowId == row,
                              self.activeKey == item.key, self.activeItem == id else { return }
                        self.pageText = message
                    }
                }
            }
        }
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
        guard let index = tabButtons.firstIndex(of: sender) else { return }
        activate(docs[index].key, focus: true)
        returnFocus()
    }

    @objc private func addDoc() {
        urlField.isHidden.toggle()
        view.needsLayout = true
        if !urlField.isHidden { view.window?.makeFirstResponder(urlField) }
    }

    @objc private func commitURL() {
        let raw = urlField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty, rowId != nil else { return }
        let url = raw.hasPrefix("http://") || raw.hasPrefix("https://") ? URL(string: raw) : URL(fileURLWithPath: raw)
        guard let url else { return }
        windowController?.openOnDesk(url, paneId: nil)
        urlField.stringValue = ""
        urlField.isHidden = true
    }

    private func registerDocHooks() {
        ClickRegistry.shared.set("+") { [weak self] in self?.addDoc() }
        ClickRegistry.shared.set("✕") { [weak self] in self?.closed() }
        for item in docs {
            let title = item.title
            ClickRegistry.shared.set("doc_tab:\(title)") { [weak self] in
                guard let self else { return }
                self.activate(item.key, focus: true)
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
        var deskId: String?
        var mime: String? = nil
        enum Kind: String { case web, markdown, file }
        var key: String { "\(deskId ?? "")|\(title)|\(url ?? "")|\(path ?? "")" }
    }

    static func items(model: HerdrModel, tabId: String?) -> [DocItem] {
        guard let tabId else { return [] }
        let snap = model.catalog.snapshot
        let lane = snap.lanes[tabId]
        let name = snap.displayName(tab: tabId, lane: lane, fallback: model.source(for: tabId)?.tabs.first { $0.tab_id == tabId }?.label ?? "")
        var out: [DocItem] = []
        if let u = lane?.scopeURL { out.append(DocItem(title: "Scope", kind: .web, url: u, path: nil, deskId: nil)) }
        if let u = lane?.reviewURL { out.append(DocItem(title: "Review", kind: .web, url: u, path: nil, deskId: nil)) }
        if let folder = projectFolder(scopeURL: lane?.scopeURL, displayName: name) {
            for file in ["RESUME", "BRIEF", "DECISIONS"] {
                let path = (folder as NSString).appendingPathComponent("\(file).md")
                if FileManager.default.fileExists(atPath: path) {
                    out.append(DocItem(title: file, kind: .markdown, url: nil, path: path, deskId: nil))
                }
            }
        }
        out.append(contentsOf: (model.source(for: tabId)?.tabs.first { $0.tab_id == tabId }?.desk?.items ?? []).map(docItem))
        return out
    }

    private static func docItem(_ item: DeskItem) -> DocItem {
        DocItem(title: item.title, kind: item.kind == "url" ? .web : .file,
                url: item.kind == "url" ? item.ref : nil, path: item.kind == "file" ? item.ref : nil,
                deskId: item.id, mime: item.mime)
    }

    func activate(_ key: String, focus: Bool = false) {
        guard let item = docs.first(where: { $0.key == key }) else { return }
        activeKey = key
        loadActive()
        if focus, let id = item.deskId, !id.hasPrefix("local-") { deskCommand("desk.focus", item: id) }
    }

    func activateDesk(_ id: String) {
        if let item = docs.first(where: { $0.deskId == id }) { activate(item.key) }
    }

    func addTransient(_ url: URL, tabId: String) {
        let item = DeskItem(id: "local-" + UUID().uuidString, kind: url.isFileURL ? "file" : "url",
                            ref: url.isFileURL ? url.path : url.absoluteString, title: url.isFileURL ? url.lastPathComponent : url.host ?? url.absoluteString,
                            mime: Self.localMime(url), opened_by: "user", opened_at_ms: 0)
        transient[tabId, default: []].append(item)
        if let c = windowController, c.state.selectedTab == tabId {
            show(model: c.model, tabId: tabId)
            activateDesk(item.id)
        }
    }

    private static func localMime(_ url: URL) -> String {
        switch url.pathExtension.lowercased() {
        case "md", "markdown": return "text/markdown"
        case "html", "htm": return "text/html"
        case "pdf": return "application/pdf"
        case "png": return "image/png"
        case "jpg", "jpeg": return "image/jpeg"
        case "gif": return "image/gif"
        case "webp": return "image/webp"
        case "svg": return "image/svg+xml"
        default: return "text/plain"
        }
    }

    private func deskCommand(_ method: String, item: String) {
        guard let rowId, let cmds = windowController?.commands else { return }
        DispatchQueue.global(qos: .userInitiated).async { _ = cmds.deskCall(method, params: ["tab_id": rowId, "item": item]) }
    }

    private func readDeskFile(_ item: DocItem) {
        guard !readInFlight, let rowId, let id = item.deskId, let cmds = windowController?.commands else { return }
        readInFlight = true
        let generation = readGeneration
        if id.hasPrefix("local-"), let path = item.path {
            let previousMtime = watchMtime, needsData = fileData == nil
            DispatchQueue.global(qos: .utility).async { [weak self] in
                let mtime = Self.mtime(path)
                let data = needsData || mtime != previousMtime ? try? Data(contentsOf: URL(fileURLWithPath: path)) : nil
                DispatchQueue.main.async {
                    guard let self, generation == self.readGeneration, self.rowId == rowId,
                          self.activeItem == id else { return }
                    self.readInFlight = false
                    self.watchMtime = mtime
                    if let data { self.renderFile(data, mime: item.mime ?? "text/plain", item: item) }
                }
            }
            return
        }
        var params: [String: Any] = ["tab_id": rowId, "item": id]
        if let fileMtime { params["known_mtime_ms"] = fileMtime }
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let result = cmds.deskCall("desk.read", params: params)
            DispatchQueue.main.async {
                guard let self else { return }
                guard generation == self.readGeneration, self.rowId == rowId, self.activeItem == id else { return }
                self.readInFlight = false
                // A poll that fails while a file is already shown (the server mid-handoff) keeps it.
                guard let result else {
                    if self.fileData == nil { self.pageText = "Unable to read desk file" }
                    return
                }
                self.fileMtime = (result["mtime_ms"] as? NSNumber)?.uint64Value
                guard result["unchanged"] as? Bool != true,
                      let raw = result["data_base64"] as? String, let data = Data(base64Encoded: raw) else { return }
                self.renderFile(data, mime: result["mime"] as? String ?? item.mime ?? "text/plain", item: item)
            }
        }
    }

    private func renderFile(_ data: Data, mime: String, item: DocItem) {
        fileData = data
        let base = item.path.map { URL(fileURLWithPath: $0).deletingLastPathComponent() }
        if mime == "text/markdown" {
            web.loadHTMLString(MiniMarkdown.html(String(data: data, encoding: .utf8) ?? "", title: item.title), baseURL: base)
        } else if mime == "text/plain" {
            let text = (String(data: data, encoding: .utf8) ?? "").replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
            web.loadHTMLString("<pre>" + text + "</pre>", baseURL: base)
        } else {
            web.load(data, mimeType: mime, characterEncodingName: "utf-8", baseURL: base ?? URL(fileURLWithPath: "/"))
        }
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

/// The column's header shares the transparent titlebar's band. A view that can move the
/// window marks that band as a drag region, and the window server then takes real clicks on
/// the tabs, + and ✕ as window drags; so the column never moves the window.
final class DocPanelView: NSView {
    var onLayout: ((NSRect) -> Void)?
    var onAddressChord: (() -> Bool)?
    override var isFlipped: Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
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
    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

final class DocWidthHandle: NSView {
    var onDrag: ((CGFloat) -> Void)?
    var current: CGFloat = DocPanelController.defaultWidth
    private var origin: CGFloat = 0
    private var start: CGFloat = 0

    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
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
