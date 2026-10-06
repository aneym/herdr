import AppKit
import Combine
import SwiftUI

// One token set for the whole shell (P5, Alex 2026-09-28).
//
//  - Light and dark are both first-class. The effective mode is the macOS appearance,
//    followed live, unless an explicit override says otherwise.
//  - Chrome colors and the terminal palette come from the same place: the chrome
//    tokens below, and the terminal background and foreground read from the very
//    Ghostty theme pair the surfaces load (dark:X,light:Y). A switch flips both.
//  - Glass (blur or vibrancy) is a per-surface token, off by default, and only the
//    sidebar and transient overlays have one. Terminal panes and reading surfaces
//    have no glass token at all and stay opaque.
//  - No edge-stripe cards: emphasis is type weight, a full hairline border, or a
//    tinted fill, never a colored bar down one edge.

enum Mode: String, CaseIterable {
    case light, dark
}

enum AppearanceOverride: String {
    case system, light, dark
}

/// Chrome colors for one mode. Every text token (ink, mute, orch, lane, wf, ok, warn)
/// is checked at 4.5:1 against panel and sel by scripts/contrast.py.
struct ChromePalette: Equatable {
    var windowBg: UInt32
    var panel: UInt32
    var line: UInt32
    var sel: UInt32
    var ink: UInt32
    var mute: UInt32
    var orch: UInt32
    var lane: UInt32
    var wf: UInt32
    var ok: UInt32
    var warn: UInt32
    var cap: UInt32
    var hover: UInt32
    var split: UInt32
    var field: UInt32
    var faint: UInt32
    var accent: UInt32
    var bad: UInt32

    static let textTokenNames = ["ink", "mute", "orch", "lane", "wf", "ok", "warn"]
    static let surfaceTokenNames = ["panel", "sel"]

    /// The mock's palette.
    static let dark = ChromePalette(
        windowBg: 0x0F1013, panel: 0x16171B, line: 0x26272D, sel: 0x1F2230,
        ink: 0xE6E6EA, mute: 0x8B8C94, orch: 0x5AA9FF, lane: 0xA3AAFF, wf: 0x5CC8C8,
        ok: 0x4CD27A, warn: 0xF2B04C,
        cap: 0x181825, hover: 0x222232, split: 0x3B3D54, field: 0x24253A,
        faint: 0x6C7086, accent: 0x89B4FA, bad: 0xF38BA8)

    /// Paper-toned counterpart: same hues, darkened until each text token clears 4.5:1.
    static let light = ChromePalette(
        windowBg: 0xE9EBEF, panel: 0xF3F4F6, line: 0xD5D8DE, sel: 0xE1E6F2,
        ink: 0x1F2328, mute: 0x565C66, orch: 0x0A5FC4, lane: 0x4A4FC7, wf: 0x0B6E74,
        ok: 0x17692F, warn: 0x8A5300,
        cap: 0xF1F1EE, hover: 0xF0F0ED, split: 0xD4D4CF, field: 0xF2F2EF,
        faint: 0xA3A8AF, accent: 0x0969DA, bad: 0xCF222E)

    /// Text tokens for the sidebar when its glass token is on. Glass shows an arbitrary
    /// desktop through a `glassScrimAlpha` panel tint, so the panel behind the text can
    /// be as light as the panel over white or as dark as the panel over black. These
    /// are the base tokens nudged (lightness only) until every one clears 4.6:1 on both
    /// extremes and on the opaque `sel` row fill. scripts/contrast.py checks them.
    var glassText: ChromePalette {
        var p = self
        if panel == ChromePalette.dark.panel && ink == ChromePalette.dark.ink {
            p.mute = 0x9B9CA3
        } else if panel == ChromePalette.light.panel && ink == ChromePalette.light.ink {
            p.orch = 0x0959B8; p.lane = 0x484DC6; p.wf = 0x0A656B; p.warn = 0x855000
        }
        return p
    }

    /// Opacity of the panel-colored tint laid over the sidebar's glass layer.
    static let glassScrimAlpha: Double = 0.88

    func value(_ name: String) -> UInt32? {
        switch name {
        case "windowBg": return windowBg
        case "panel": return panel
        case "line": return line
        case "sel": return sel
        case "ink": return ink
        case "mute": return mute
        case "orch": return orch
        case "lane": return lane
        case "wf": return wf
        case "ok": return ok
        case "warn": return warn
        case "cap": return cap
        case "hover": return hover
        case "split": return split
        case "field": return field
        case "faint": return faint
        case "accent": return accent
        case "bad": return bad
        default: return nil
        }
    }

    /// sRGB mix: `a` toward `b` by `t`.
    static func mix(_ a: UInt32, _ b: UInt32, _ t: Double) -> UInt32 {
        func ch(_ v: UInt32, _ shift: UInt32) -> Double { Double((v >> shift) & 0xFF) }
        func pack(_ r: Double, _ g: Double, _ b: Double) -> UInt32 {
            func c(_ v: Double) -> UInt32 { UInt32(min(255, max(0, Int(v.rounded())))) }
            return (c(r) << 16) | (c(g) << 8) | c(b)
        }
        let t = min(1, max(0, t))
        return pack(ch(a, 16) * (1 - t) + ch(b, 16) * t,
                    ch(a, 8) * (1 - t) + ch(b, 8) * t,
                    ch(a, 0) * (1 - t) + ch(b, 0) * t)
    }
}

/// Per-surface glass. Only the sidebar and transient overlays (command palette,
/// toasts, overview) have a token. Default off.
struct GlassTokens: Equatable {
    var sidebar = false
    var overlay = false
    var any: Bool { sidebar || overlay }
}

/// Everything a view needs for one mode.
struct Tokens {
    let mode: Mode
    let chrome: ChromePalette
    let terminalBg: UInt32
    let terminalFg: UInt32

    var windowBg: Color { Color(hex: chrome.windowBg) }
    var panel: Color { Color(hex: chrome.panel) }
    var line: Color { Color(hex: chrome.line) }
    var sel: Color { Color(hex: chrome.sel) }
    var ink: Color { Color(hex: chrome.ink) }
    var mute: Color { Color(hex: chrome.mute) }
    var orch: Color { Color(hex: chrome.orch) }
    var lane: Color { Color(hex: chrome.lane) }
    var wf: Color { Color(hex: chrome.wf) }
    var ok: Color { Color(hex: chrome.ok) }
    var warn: Color { Color(hex: chrome.warn) }
    var cap: Color { Color(hex: chrome.cap) }
    var hover: Color { Color(hex: chrome.hover) }
    var split: Color { Color(hex: chrome.split) }
    var field: Color { Color(hex: chrome.field) }
    var faint: Color { Color(hex: chrome.faint) }
    var accent: Color { Color(hex: chrome.accent) }
    var bad: Color { Color(hex: chrome.bad) }
    /// Ink at a few percent: toggle tracks and small tags.
    var tint: Color { ink.opacity(mode == .dark ? 0.055 : 0.045) }

    var terminalBgNS: NSColor { NSColor(hex: terminalBg) }
    var windowBgNS: NSColor { NSColor(hex: terminalBg) }
    var splitNS: NSColor { NSColor(hex: chrome.split) }
    var capNS: NSColor { NSColor(hex: chrome.cap) }
    var accentNS: NSColor { NSColor(hex: chrome.accent) }
    var inkNS: NSColor { NSColor(hex: chrome.ink) }
}

extension Color {
    init(hex: UInt32) {
        self.init(red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255, blue: Double(hex & 0xFF) / 255)
    }
}

extension NSColor {
    /// sRGB, so it matches what Ghostty draws for the same hex.
    convenience init(hex: UInt32) {
        self.init(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                  blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}

// MARK: Ghostty config and theme pair

/// The terminal side of the token set. Builds the config the surfaces load and reads
/// the background and foreground of the theme pair from the theme files.
struct TerminalTheme {
    var dark: String
    var light: String
    var darkColors: (bg: UInt32, fg: UInt32)
    var lightColors: (bg: UInt32, fg: UInt32)

    static let fallbackDark: (bg: UInt32, fg: UInt32) = (0x1E1E2E, 0xCDD6F4)
    static let fallbackLight: (bg: UInt32, fg: UInt32) = (0xFFFFFF, 0x3A3A38)

    /// Directive lines from Alex's own config that must never reach a surface: keybinds
    /// (the shell owns chords through its menu and keymap), includes, and anything that
    /// would make a terminal pane translucent or change the shell's grid padding.
    static let strippedKeys: Set<String> = [
        "keybind", "config-file", "background-opacity", "background-blur", "background-blur-radius",
        "background-opacity-cells",
        "window-padding-x", "window-padding-y", "window-padding-balance",
    ]

    static func key(of line: String) -> String? {
        let t = line.trimmingCharacters(in: .whitespaces)
        if t.isEmpty || t.hasPrefix("#") { return nil }
        guard let eq = t.firstIndex(of: "=") else { return t }
        return t[..<eq].trimmingCharacters(in: .whitespaces)
    }

    /// Base defaults, then the user's config minus stripped keys, then enforced lines.
    static func mergedConfig(base: String, user: String?) -> String {
        var out = base.components(separatedBy: "\n")
        if let user {
            for line in user.components(separatedBy: "\n") {
                if let k = key(of: line), strippedKeys.contains(k) { continue }
                out.append(line)
            }
        }
        out.append("background-opacity = 1")
        out.append("window-padding-x = 8")
        out.append("window-padding-y = 6")
        out.append("window-padding-balance = false")
        return out.joined(separator: "\n") + "\n"
    }

    static func realHome() -> String {
        if let pw = getpwuid(getuid()), let d = pw.pointee.pw_dir { return String(cString: d) }
        return NSHomeDirectory()
    }

    static func parse(config: String, resources: String) -> TerminalTheme {
        var value = "dark:Catppuccin Mocha,light:Catppuccin Latte"
        for line in config.components(separatedBy: "\n") where key(of: line) == "theme" {
            if let eq = line.firstIndex(of: "=") {
                value = line[line.index(after: eq)...].trimmingCharacters(in: .whitespaces)
            }
        }
        var dark = value, light = value
        if value.hasPrefix("dark:") || value.hasPrefix("light:") {
            // "dark:A,light:B" in either order. Theme names may contain spaces but not
            // ",light:" or ",dark:".
            for part in value.components(separatedBy: ",") {
                if part.hasPrefix("dark:") { dark = String(part.dropFirst(5)) }
                if part.hasPrefix("light:") { light = String(part.dropFirst(6)) }
            }
        }
        func colors(_ theme: String, fallback: (bg: UInt32, fg: UInt32)) -> (bg: UInt32, fg: UInt32) {
            let home = realHome()
            let candidates = theme.hasPrefix("/")
                ? [theme]
                : [home + "/.config/ghostty/themes/" + theme, resources + "/themes/" + theme]
            for p in candidates {
                guard let text = try? String(contentsOfFile: p, encoding: .utf8) else { continue }
                var bg: UInt32?, fg: UInt32?
                for l in text.components(separatedBy: "\n") {
                    guard let k = key(of: l), let eq = l.firstIndex(of: "=") else { continue }
                    let v = l[l.index(after: eq)...].trimmingCharacters(in: .whitespaces)
                    let hex = v.hasPrefix("#") ? String(v.dropFirst()) : v
                    if k == "background" { bg = UInt32(hex, radix: 16) }
                    if k == "foreground" { fg = UInt32(hex, radix: 16) }
                }
                if let bg, let fg { return (bg, fg) }
            }
            log("theme '\(theme)' not found; using built-in colors for chrome (Ghostty will use its default)")
            return fallback
        }
        return TerminalTheme(dark: dark, light: light,
                             darkColors: colors(dark, fallback: fallbackDark),
                             lightColors: colors(light, fallback: fallbackLight))
    }

    func colors(_ m: Mode) -> (bg: UInt32, fg: UInt32) { m == .dark ? darkColors : lightColors }
}

// MARK: Store

/// Live theme state: system appearance, explicit override, glass tokens.
final class ThemeStore: ObservableObject {
    @Published var override: AppearanceOverride
    @Published private(set) var system: Mode
    @Published var glass: GlassTokens
    let terminal: TerminalTheme
    private var kvo: NSKeyValueObservation?

    init(override: AppearanceOverride, glass: GlassTokens, terminal: TerminalTheme) {
        self.override = override
        self.glass = glass
        self.terminal = terminal
        self.system = ThemeStore.currentSystemMode()
    }

    static func currentSystemMode() -> Mode {
        let best = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua])
        return best == .darkAqua ? .dark : .light
    }

    /// Follow the macOS appearance with no restart. NSApp.appearance stays nil, so its
    /// effectiveAppearance is always the system's, whatever the window override says.
    func startFollowingSystem() {
        kvo = NSApp.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
            DispatchQueue.main.async {
                guard let self else { return }
                let m = ThemeStore.currentSystemMode()
                if m != self.system { self.system = m }
            }
        }
    }

    var effective: Mode {
        switch override {
        case .system: return system
        case .light: return .light
        case .dark: return .dark
        }
    }

    var tokens: Tokens { ThemeStore.tokens(for: effective, terminal: terminal) }

    /// Tokens the sidebar draws with: the glass text palette while sidebar glass is on.
    var sidebarTokens: Tokens {
        let t = tokens
        return glass.sidebar ? Tokens(mode: t.mode, chrome: t.chrome.glassText, terminalBg: t.terminalBg, terminalFg: t.terminalFg) : t
    }

    static func tokens(for mode: Mode, terminal: TerminalTheme) -> Tokens {
        let c = terminal.colors(mode)
        var chrome: ChromePalette = mode == .dark ? .dark : .light
        let name = mode == .dark ? terminal.dark : terminal.light
        let stock = (mode == .dark && (name == "Catppuccin Mocha" || c.bg == 0x1E1E2E))
            || (mode == .light && (name == "SF Paper" || name == "Catppuccin Latte" || c.bg == 0xFFFFFF || c.bg == TerminalTheme.fallbackLight.bg))
        if !stock {
            chrome = derived(bg: c.bg, fg: c.fg, mode: mode, base: chrome)
        }
        chrome.windowBg = c.bg
        chrome.split = ChromePalette.mix(c.bg, c.fg, 0.16)
        if mode == .light { chrome.cap = ChromePalette.mix(c.bg, c.fg, 0.06) }
        return Tokens(mode: mode, chrome: chrome, terminalBg: c.bg, terminalFg: c.fg)
    }

    /// Chrome for a Ghostty theme that is not the stock pair. Text colors stay on `base`
    /// so the contrast gate keeps its pairs; the surfaces that sit on the terminal move with it.
    private static func derived(bg: UInt32, fg: UInt32, mode: Mode, base: ChromePalette) -> ChromePalette {
        var p = base
        p.windowBg = bg
        p.panel = mode == .dark ? ChromePalette.mix(bg, 0x000000, 0.20) : ChromePalette.mix(bg, fg, 0.035)
        p.cap = mode == .dark ? p.panel : ChromePalette.mix(bg, fg, 0.06)
        p.hover = ChromePalette.mix(p.panel, fg, 0.04)
        p.sel = ChromePalette.mix(p.panel, fg, 0.09)
        p.line = ChromePalette.mix(bg, fg, 0.08)
        p.split = ChromePalette.mix(bg, fg, 0.16)
        p.field = ChromePalette.mix(bg, fg, 0.04)
        return p
    }

    var nsAppearance: NSAppearance? {
        switch override {
        case .system: return nil
        case .light: return NSAppearance(named: .aqua)
        case .dark: return NSAppearance(named: .darkAqua)
        }
    }

    /// JSON for scripts/contrast.py and the app's state dump.
    static func dump(terminal: TerminalTheme) -> [String: Any] {
        var modes: [String: Any] = [:]
        var glassModes: [String: Any] = [:]
        for m in Mode.allCases {
            let t = tokens(for: m, terminal: terminal)
            var d: [String: String] = [:]
            for n in ["windowBg", "panel", "line", "sel"] + ChromePalette.textTokenNames { d[n] = hex(t.chrome.value(n)!) }
            d["terminalBg"] = hex(t.terminalBg)
            d["terminalFg"] = hex(t.terminalFg)
            modes[m.rawValue] = d
            var g: [String: String] = [:]
            for n in ["panel", "sel"] + ChromePalette.textTokenNames { g[n] = hex(t.chrome.glassText.value(n)!) }
            glassModes[m.rawValue] = g
        }
        return ["modes": modes,
                "glass_modes": glassModes,
                "glass_scrim_alpha": ChromePalette.glassScrimAlpha,
                "text_tokens": ChromePalette.textTokenNames,
                "surface_tokens": ChromePalette.surfaceTokenNames,
                "terminal_theme": ["dark": terminal.dark, "light": terminal.light]]
    }

    static func hex(_ v: UInt32) -> String { String(format: "#%06X", v) }
}

// MARK: Glass

/// A SwiftUI background for transient overlays (command palette, toasts, overview).
/// Opaque panel color unless the overlay glass token is on.
struct OverlayBackground: View {
    @ObservedObject var theme: ThemeStore
    var corner: CGFloat = 10

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: corner)
        if theme.glass.overlay {
            if #available(macOS 26.0, *) {
                Color.clear.glassEffect(.regular, in: shape)
            } else {
                shape.fill(.ultraThinMaterial)
            }
        } else {
            shape.fill(theme.tokens.panel).overlay(shape.stroke(theme.tokens.line, lineWidth: 1))
        }
    }
}

/// The sidebar column: an optional glass layer under the SwiftUI content.
final class SidebarContainer: NSView {
    let content: NSView
    private(set) var effect: NSView?
    private(set) var glassOn = false

    init(content: NSView) {
        self.content = content
        super.init(frame: .zero)
        wantsLayer = true
        addSubview(content)
    }

    required init?(coder: NSCoder) { fatalError() }

    func apply(glass: Bool, panel: NSColor) {
        glassOn = glass
        if glass {
            if effect == nil {
                let e: NSView
                if #available(macOS 26.0, *) {
                    let g = NSGlassEffectView()
                    g.cornerRadius = 0
                    e = g
                } else {
                    let v = NSVisualEffectView()
                    v.material = .sidebar
                    v.blendingMode = .behindWindow
                    v.state = .active
                    e = v
                }
                addSubview(e, positioned: .below, relativeTo: content)
                effect = e
            }
            layer?.backgroundColor = nil
        } else {
            effect?.removeFromSuperview()
            effect = nil
            layer?.backgroundColor = panel.cgColor
        }
        needsLayout = true
    }

    var effectKind: String { effect.map { String(describing: type(of: $0)) } ?? "none" }

    override func layout() {
        super.layout()
        effect?.frame = bounds
        content.frame = bounds
    }
}
