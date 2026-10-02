import AppKit
import GhosttyKit

// Usage: HerdrShell --herdr <bin> --socket <herdr api socket> [--control <fifo>]
//                   [--ghostty-config <file>] [--user-ghostty-config <file>]
//                   [--appearance system|light|dark] [--glass sidebar,overlay] [--hosts-stub FILE] [--allow-live]
//        HerdrShell --dump-tokens      (print the theme tokens as JSON and exit)
//        HerdrShell --demo factory [--dump-factory <json>] [--appearance light|dark]
//        HerdrShell --demo chat --pane <pane> [--read-only] [--control <fifo>]   (P19 chat for one pane)
//        HerdrShell --transcript <file> [--dump-chat <json> (also <json>.png, <json>.view.json)] [--chat-state working|blocked|asleep] [--agent-name <n>]
//        (either chat: [--chat-mode focus|full] pins the mode instead of the saved one)
var args: [String: String] = [:]
var flags = Set<String>()
do {
    var it = CommandLine.arguments.dropFirst().makeIterator()
    while let a = it.next() {
        if a == "--allow-live" || a == "--dump-tokens" || a == "--agent-run" || a == "--read-only"
            || a == "--selftest-channel" || a == "--update-helper" {
            flags.insert(a); continue
        }
        if a.hasPrefix("--"), let v = it.next() { args[String(a.dropFirst(2))] = v }
    }
}
if flags.contains("--update-helper") {
    UpdateRestart.helperMain(args: args)
}
if Channel.kind == .dev && flags.contains("--allow-live") {
    log("refusing: dev channel does not take --allow-live")
    exit(2)
}
if Channel.kind == .prod && (args["control"] != nil || args["chat-hook"] != nil) {
    log("refusing: prod channel does not take --control or a chat hook")
    exit(2)
}
if flags.contains("--selftest-channel") {
    Channel.printSelfTest()
    exit(0)
}
if let dir = args["selftest-update"] {
    UpdateSelfTest.run(dir)
}
let pkgRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().path

// Terminal side of the token set: the shell's defaults, then Alex's Ghostty config with
// every keybind stripped. Built before anything else so --dump-tokens needs no session.
// Ghostty resources (themes, shell integration): the flag, then the package's own
// Resources/ghostty, then an installed Ghostty.app, then the vendored build tree.
let resourcesDir: String = {
    var candidates: [String] = []
    if let bundled = Bundle.main.resourceURL?.appendingPathComponent("ghostty").path { candidates.append(bundled) }
    if let flag = args["ghostty-resources"] { candidates.append(flag) }
    candidates.append(contentsOf: [
        pkgRoot + "/Resources/ghostty",
        "/Applications/Ghostty.app/Contents/Resources/ghostty",
        pkgRoot + "/../vendor/ghostty/zig-out/share/ghostty",
        "/Volumes/StudioExt/repos/herdr-shell-spikes/vendor/ghostty/zig-out/share/ghostty",
    ])
    let paths = candidates.map { ($0 as NSString).standardizingPath }
    return paths.first { FileManager.default.fileExists(atPath: $0 + "/themes") } ?? paths[0]
}()
let baseConfig = (try? String(contentsOfFile: args["ghostty-config"] ?? shellResource("ghostty.conf"), encoding: .utf8)) ?? ""
let userConfigPath = args["user-ghostty-config"] ?? (TerminalTheme.realHome() + "/.config/ghostty/config")
let userConfig = try? String(contentsOfFile: userConfigPath, encoding: .utf8)
let ghosttyConfigText = TerminalTheme.mergedConfig(base: baseConfig, user: userConfig)
let terminalTheme = TerminalTheme.parse(config: ghosttyConfigText, resources: resourcesDir)
if flags.contains("--dump-tokens") {
    let data = try! JSONSerialization.data(withJSONObject: ThemeStore.dump(terminal: terminalTheme), options: [.prettyPrinted, .sortedKeys])
    print(String(decoding: data, as: UTF8.self))
    exit(0)
}
let appearanceOverride = AppearanceOverride(rawValue: args["appearance"] ?? "system") ?? .system
var glassTokens = GlassTokens()
for s in (args["glass"] ?? "").split(separator: ",") {
    if s == "sidebar" { glassTokens.sidebar = true }
    if s == "overlay" { glassTokens.overlay = true }
}
if args["demo"] == "factory" {
    runFactoryDemo()
}
if args["demo"] == "chat" || args["transcript"] != nil {
    if args["transcript"] == nil && args["pane"] == nil { log("chat requires --pane or --transcript"); exit(2) }
    if args["transcript"] != nil && args["pane"] != nil && !flags.contains("--read-only") {
        log("refusing --transcript \(args["transcript"]!) --pane \(args["pane"]!) without --read-only")
        exit(2)
    }
    if let bin = args["herdr"] { setenv("HERDR_BIN", bin, 1) }
    if let socket = args["socket"] { setenv("HERDR_SOCKET_PATH", socket, 1) }
    let chatApp = NSApplication.shared
    chatApp.setActivationPolicy(ChatDemoDelegate.offscreen ? .prohibited : .regular)
    let chatDelegate = ChatDemoDelegate()
    chatApp.delegate = chatDelegate
    withExtendedLifetime(chatDelegate) { chatApp.run() }
    exit(0)
}
var herdrBin = args["herdr"] ?? ""
var socket = args["socket"] ?? ""
if Channel.kind == .prod {
    if herdrBin.isEmpty { herdrBin = (Channel.home as NSString).appendingPathComponent(".local/bin/herdr") }
    if socket.isEmpty { socket = (Channel.home as NSString).appendingPathComponent(".config/herdr/herdr.sock") }
    flags.insert("--allow-live")
}
if herdrBin.isEmpty || socket.isEmpty {
    log("usage: HerdrShell --herdr <bin> --socket <api socket> [--control <fifo>]")
    exit(2)
}
// Safety: the spike never attaches to the live default session unless told to.
let liveSocket = NSHomeDirectory() + "/.config/herdr/herdr.sock"
if (socket == liveSocket || socket.hasSuffix("/.config/herdr/herdr.sock")) && !flags.contains("--allow-live") {
    log("refusing: \(socket) looks like the live session socket (pass --allow-live)")
    exit(2)
}

// P15 fixtures. Read before HERDR_* is cleared; the lab launcher forwards these two.
let home = ProcessInfo.processInfo.environment["HOME"] ?? NSHomeDirectory()
let shellLanesPath = ProcessInfo.processInfo.environment["HERDR_LANES_PATH"] ?? (home + "/.agent-rails/herdr/lanes.json")
let shellAreasPath = ProcessInfo.processInfo.environment["HERDR_AREAS_PATH"] ?? (home + "/.agent-rails/herdr/areas.json")
ShellPaths.lanes = shellLanesPath
ShellPaths.areas = shellAreasPath
ContextStore.directory = ProcessInfo.processInfo.environment["HERDR_CONTEXT_DIR"]
    ?? Channel.appSupport.appendingPathComponent("context").path

// Children must not inherit the launching pane's herdr identity (HERDR_ENV and
// friends) or any Claude session markers.
for (k, _) in ProcessInfo.processInfo.environment where k.hasPrefix("HERDR_") || k.hasPrefix("CLAUDE") {
    unsetenv(k)
}
setenv("HERDR_SOCKET_PATH", socket, 1)
setenv("GHOSTTY_RESOURCES_DIR", resourcesDir, 1)

var argv0: [UnsafeMutablePointer<CChar>?] = [strdup(CommandLine.arguments[0]), nil]
guard ghostty_init(1, &argv0) == GHOSTTY_SUCCESS else { log("ghostty_init failed"); exit(1) }

let appStart = Date()
/// Checks pass this so the window stays offscreen and never becomes the front app.
let agentRun = flags.contains("--agent-run")
let app = NSApplication.shared
app.setActivationPolicy(agentRun ? .accessory : .regular)

final class AppDelegate: NSObject, NSApplicationDelegate {
    var controller: MainWindowController!
    var hook: TestHook?
    var model: HerdrModel!
    var theme: ThemeStore!

    func applicationDidFinishLaunching(_ note: Notification) {
        let t0 = Date()
        theme = ThemeStore(override: appearanceOverride, glass: glassTokens, terminal: terminalTheme)
        theme.startFollowingSystem()
        GhosttyRuntime.shared = GhosttyRuntime(configText: ghosttyConfigText)
        let env = ProcessInfo.processInfo.environment
        let hostsProvider: HostsProvider = args["hosts-stub"].map { FileHostsProvider(path: $0) } ?? HerdrOnlyHostsProvider()
        model = HerdrModel(herdrBin: herdrBin, env: env, hostsProvider: hostsProvider)
        let registry = SurfaceRegistry(herdrBin: herdrBin, attachEnv: ["HERDR_SOCKET_PATH": socket])
        controller = MainWindowController(model: model, registry: registry, theme: theme)
        NSApp.mainMenu = buildMenu(target: controller)
        controller.show()
        model.start()
        if let fifo = args["control"], Channel.kind == .dev {
            hook = TestHook(path: fifo, controller: controller)
            hook?.start()
        }
        if Channel.kind == .prod { controller.updates?.start() }
        log(String(format: "launched in %.0f ms (window %d)", Date().timeIntervalSince(t0) * 1000, controller.window.windowNumber))
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

func buildMenu(target: MainWindowController) -> NSMenu {
    let main = NSMenu()
    func submenu(_ title: String, _ items: [NSMenuItem]) {
        let m = NSMenu(title: title)
        items.forEach { m.addItem($0) }
        let top = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        top.submenu = m
        main.addItem(top)
    }
    func item(_ title: String, _ sel: Selector, _ key: String, _ mods: NSEvent.ModifierFlags = [.command], tag: Int = 0) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: sel, keyEquivalent: key)
        i.keyEquivalentModifierMask = mods
        i.target = target
        i.tag = tag
        return i
    }
    let quit = NSMenuItem(title: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
    var appItems = [quit]
    if Channel.kind == .prod {
        appItems.insert(item("Check for Updates…", #selector(MainWindowController.checkForUpdates(_:)), "u", [.command, .shift]), at: 0)
    }
    submenu(Channel.name, appItems)
    submenu("Edit", [
        item("Copy", #selector(MainWindowController.copy(_:)), "c"),
        item("Paste", #selector(MainWindowController.paste(_:)), "v"),
    ])
    // Every app chord comes from Resources/keymap.json; the menu only mirrors it.
    Keymap.shared.dispatcher = { [weak target] in target?.perform(action: $0) ?? false }
    for (title, items) in keymapMenus(target: target) { submenu(title, items) }
    return main
}

let delegate = AppDelegate()
app.delegate = delegate
app.run()

func runFactoryDemo() -> Never {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    let demo = FactoryDemo()
    app.delegate = demo
    app.run()
    exit(0)
}

final class FactoryDemo: NSObject, NSApplicationDelegate {
    let theme = ThemeStore(override: appearanceOverride, glass: glassTokens, terminal: terminalTheme)
    let model = FactoryModel(dumpPath: args["dump-factory"])
    var window: NSWindow!
    private var shot = false
    private var started: Date?

    func applicationDidFinishLaunching(_ notification: Notification) {
        started = Date()
        theme.startFollowingSystem()
        let host = FactoryDemoUI.hosting(model: model, theme: theme)
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1100, height: 900),
                           styleMask: [.titled, .closable, .miniaturizable, .resizable],
                           backing: .buffered, defer: false)
        window.title = "Factory"
        window.appearance = theme.nsAppearance
        window.contentView = host
        window.setContentSize(NSSize(width: 1100, height: 900))
        window.center()
        window.backgroundColor = theme.tokens.windowBgNS
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        let menu = NSMenu()
        let appMenu = NSMenuItem()
        menu.addItem(appMenu)
        let sub = NSMenu()
        sub.addItem(NSMenuItem(title: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        appMenu.submenu = sub
        NSApp.mainMenu = menu
        model.start()
        Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { [weak self] timer in
            MainActor.assumeIsolated { self?.capture(timer) }
        }
        log("factory demo")
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    @MainActor private func capture(_ timer: Timer) {
        guard !shot, let dump = args["dump-factory"] else { return }
        let waited = Date().timeIntervalSince(started ?? Date())
        let pageReady = !model.snapshot.machines.isEmpty || !model.snapshot.pools.isEmpty
        if (!pageReady || !model.routingReady || !model.flightsReady || !model.landedReady) && waited < 20 { return }
        shot = true
        timer.invalidate()
        let png = URL(fileURLWithPath: dump).deletingPathExtension().appendingPathExtension("png").path
        _ = FactoryShot.write(model.snapshot, tokens: theme.tokens, to: png)
    }
}
