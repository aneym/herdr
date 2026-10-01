import AppKit

/// Right-hand docs column (P15 chrome, P16 fills it). Resizable, default 420, minimum 320.
/// It does not take keyboard focus until a later piece puts a web view in it.
final class DocPanelController: NSObject {
    static let defaultWidth: CGFloat = 420
    static let minWidth: CGFloat = 320

    let view = DocPanelView()
    var onClose: (() -> Void)?
    var onWidth: ((CGFloat) -> Void)?

    private let handle = DocWidthHandle()
    private let title = NSTextField(labelWithString: "Docs")
    private let close = NoFocusButton()

    override init() {
        super.init()
        view.wantsLayer = true
        view.onLayout = { [weak self] bounds in self?.layout(in: bounds) }
        title.font = .systemFont(ofSize: 12, weight: .medium)
        title.textColor = .secondaryLabelColor
        close.title = "✕"
        close.isBordered = false
        close.font = .systemFont(ofSize: 12)
        close.target = self
        close.action = #selector(closed)
        handle.onDrag = { [weak self] width in self?.onWidth?(width) }
        view.addSubview(title)
        view.addSubview(close)
        view.addSubview(handle)
    }

    func apply(panel: NSColor, ink: NSColor) {
        view.layer?.backgroundColor = panel.cgColor
        title.textColor = ink.withAlphaComponent(0.7)
        close.contentTintColor = ink
    }

    func layout(in bounds: NSRect) {
        close.frame = NSRect(x: bounds.width - 28, y: 6, width: 22, height: 22)
        title.frame = NSRect(x: 14, y: 8, width: bounds.width - 48, height: 16)
        handle.frame = NSRect(x: 0, y: 0, width: 6, height: bounds.height)
        handle.current = bounds.width
    }

    @objc private func closed() { onClose?() }
}

final class DocPanelView: NSView {
    var onLayout: ((NSRect) -> Void)?
    override var isFlipped: Bool { true }
    override func layout() {
        super.layout()
        onLayout?(bounds)
    }
}

/// Clicks inside the panel must not move keyboard focus off the pane (P15; P16 allows a click to).
final class NoFocusButton: NSButton {
    override var acceptsFirstResponder: Bool { false }
}

final class DocWidthHandle: NSView {
    var onDrag: ((CGFloat) -> Void)?
    var current: CGFloat = DocPanelController.defaultWidth
    private var origin: CGFloat = 0
    private var start: CGFloat = 0

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .resizeLeftRight)
    }

    override func mouseDown(with event: NSEvent) {
        origin = event.locationInWindow.x
        start = current
    }

    override func mouseDragged(with event: NSEvent) {
        let dx = event.locationInWindow.x - origin
        onDrag?(max(DocPanelController.minWidth, start - dx))
    }
}
