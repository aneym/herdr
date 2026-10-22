import AppKit
import SwiftUI

final class ThoughtsCapture: ObservableObject {
    weak var owner: MainWindowController?
    @Published var isOpen = false
    @Published var text = ""
    @Published var error = ""
    @Published var saving = false
    private weak var host: NSHostingView<ThoughtsCapturePanel>?

    init(owner: MainWindowController) { self.owner = owner }

    func install(in root: RootView, theme: ThemeStore) {
        let host = NSHostingView(rootView: ThoughtsCapturePanel(model: self, theme: theme))
        host.isHidden = true
        host.autoresizingMask = [.width, .height]
        host.frame = root.bounds
        root.thoughts = host
        root.addSubview(host, positioned: .above, relativeTo: nil)
        self.host = host
    }

    func present() {
        owner?.quickSwitch.dismiss()
        isOpen = true
        error = ""
        host?.isHidden = false
        if let host {
            host.frame = owner?.root.bounds ?? .zero
            host.superview?.addSubview(host, positioned: .above, relativeTo: nil)
        }
    }

    func dismiss() {
        guard !saving else { return }
        isOpen = false
        host?.isHidden = true
        if let owner { owner.focusPane(owner.state.focusedPane ?? owner.host.rects.first?.0.paneId) }
    }

    func save() {
        guard isOpen, !saving, let owner else { return }
        saving = true
        error = ""
        owner.model.thoughts.save(text) { [weak self] ok, message in
            guard let self else { return }
            saving = false
            if ok { text = ""; dismiss() } else { error = message }
        }
    }
}

struct ThoughtsCapturePanel: View {
    @ObservedObject var model: ThoughtsCapture
    @ObservedObject var theme: ThemeStore
    @State private var height: CGFloat = ShellSpace.step24

    var body: some View {
        let t = theme.tokens
        GeometryReader { geo in
            let top = max(ShellSpace.step12, geo.size.height / 7)
            if model.isOpen {
                ZStack(alignment: .top) {
                    t.tint
                        .contentShape(Rectangle())
                        .onTapGesture { model.dismiss() }
                    VStack(alignment: .leading, spacing: ShellSpace.step12) {
                        Text("Thought")
                            .font(.system(size: ShellType.body, weight: .medium))
                            .foregroundStyle(t.ink)
                        ZStack(alignment: .topLeading) {
                            if model.text.isEmpty {
                                Text("Type or paste. Enter saves.")
                                    .font(.system(size: ShellType.body))
                                    .foregroundStyle(t.mute)
                                    .allowsHitTesting(false)
                            }
                            ComposerField(text: $model.text, height: $height, ink: NSColor(hex: t.chrome.ink),
                                          submit: { _ in model.save() }, cancel: { model.dismiss() })
                                .frame(height: min(max(height, ShellSpace.step24), geo.size.height / 3))
                                .disabled(model.saving)
                        }
                        if !model.error.isEmpty {
                            Text(model.error).font(.system(size: ShellType.body)).foregroundStyle(t.bad)
                        }
                    }
                    .padding(ShellSpace.switcherInset)
                    .frame(width: min(ShellSpace.switcherWidth, max(0, geo.size.width - ShellSpace.step24)))
                    .background(OverlayBackground(theme: theme, corner: ShellRadius.row))
                    .padding(.top, top)
                }
                .frame(width: geo.size.width, height: geo.size.height, alignment: .top)
            }
        }
    }
}
