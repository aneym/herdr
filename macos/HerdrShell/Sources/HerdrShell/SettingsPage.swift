import AppKit
import SwiftUI

/// Settings, full window (design rule 2026-10-01: settings is always full screen and shares the
/// Rails Settings chrome): a section list on the left, the section's page on the right with one
/// large title and its action beside it. Sections today: Agents. ⌘, opens and closes it, Esc closes.
final class SettingsHostingView: NSHostingView<SettingsPage> {
    override var acceptsFirstResponder: Bool { true }
    required init(rootView: SettingsPage) {
        super.init(rootView: rootView)
        // RootView sets the frame to the whole window; the page's own ideal size must not shrink it.
        sizingOptions = []
    }
    @MainActor required dynamic init?(coder: NSCoder) { fatalError() }
}

/// Toggles the server has not echoed yet, so a switch never flips back while the call is in flight.
final class SettingsState: ObservableObject {
    @Published var pending: [String: Bool] = [:]
}

struct SettingsPage: View {
    @ObservedObject var model: HerdrModel
    @ObservedObject var catalog: LaneCatalog
    @ObservedObject var theme: ThemeStore
    @ObservedObject var state: SettingsState
    var close: () -> Void
    private var t: Tokens { theme.tokens }

    var body: some View {
        let rows = SettingsAgents.rows(model.settingsInput, pending: state.pending)
        HStack(spacing: 0) {
            nav
            Rectangle().fill(t.line).frame(width: 1)
            ScrollView {
                page(rows)
                    .frame(maxWidth: ShellSpace.settingsColumn, alignment: .topLeading)
                    .padding(.horizontal, ShellSpace.settingsPadX)
                    .padding(.top, ShellSpace.settingsPadTop)
                    .padding(.bottom, ShellSpace.step32)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
            }
            .background(t.windowBg)
        }
        .foregroundStyle(t.ink)
        .onChange(of: model.settingsInput.tabs.map(\.hidden)) { _ in
            state.pending = SettingsAgents.settle(state.pending, model.settingsInput)
        }
    }

    private var nav: some View {
        VStack(alignment: .leading, spacing: ShellSpace.step4) {
            Text("Settings")
                .font(.system(size: ShellType.sectionLabel, weight: .semibold))
                .tracking(ShellType.sectionTracking)
                .foregroundStyle(t.mute)
                .padding(.horizontal, ShellSpace.step8)
                .padding(.bottom, ShellSpace.step6)
            HStack(spacing: ShellSpace.step8) {
                Text("Agents").font(.system(size: ShellType.settingsNav, weight: .medium))
                Spacer(minLength: 0)
            }
            .padding(.horizontal, ShellSpace.step8)
            .frame(height: ShellSpace.switcherRowHeight)
            .background(RoundedRectangle(cornerRadius: ShellRadius.control).fill(t.sel))
            Spacer(minLength: 0)
        }
        .padding(.horizontal, ShellSpace.step12)
        .padding(.top, ShellSpace.settingsPadTop)
        .frame(width: ShellSpace.settingsNavWidth, alignment: .topLeading)
        .frame(maxHeight: .infinity, alignment: .topLeading)
        .background(t.panel)
    }

    private func page(_ rows: [SettingsAgents.Row]) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline) {
                Text("Agents").font(.system(size: ShellType.settingsTitle, weight: .semibold))
                Spacer(minLength: ShellSpace.step8)
                Button(action: close) {
                    Text("Done").font(.system(size: ShellType.settingsRow, weight: .medium)).foregroundStyle(t.accent)
                }
                .buttonStyle(.plain)
                .keyboardShortcut(.cancelAction)
                .hookAction("settings:done", close)
            }
            Text("Shown agents sit in the Agents list. A hidden agent keeps running and keeps its place, and you can still reach it from search.")
                .font(.system(size: ShellType.settingsMeta))
                .foregroundStyle(t.mute)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, ShellSpace.step6)
                .padding(.bottom, ShellSpace.step16)
            if rows.isEmpty {
                Text("No agents yet. Right-click a pinned chat and choose Add to Agents.")
                    .font(.system(size: ShellType.settingsRow))
                    .foregroundStyle(t.mute)
                    .padding(.vertical, ShellSpace.step12)
            }
            ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                if index > 0 { Rectangle().fill(t.line).frame(height: 1).padding(.leading, ShellFace.settingsSize + ShellSpace.step12) }
                agentRow(row)
            }
        }
    }

    /// The switch and the test hook's click both land here: show or hide the pin on the server.
    private func set(_ tab: String, shown: Bool) {
        state.pending[tab] = shown
        model.setAgentHidden(tab, !shown) { ok in if !ok { state.pending[tab] = nil } }
    }

    private func agentRow(_ row: SettingsAgents.Row) -> some View {
        HStack(spacing: ShellSpace.step12) {
            SettingsFace(face: row.face, t: t)
            VStack(alignment: .leading, spacing: ShellSpace.step2) {
                Text(row.title).font(.system(size: ShellType.settingsRow, weight: .medium)).lineLimit(1)
                if !row.meta.isEmpty {
                    Text(row.meta).font(.system(size: ShellType.settingsMeta)).foregroundStyle(t.mute).lineLimit(1)
                }
            }
            Spacer(minLength: ShellSpace.step8)
            Toggle("", isOn: Binding(get: { row.shown }, set: { set(row.id, shown: $0) }))
            .toggleStyle(.switch)
            .controlSize(.small)
            .tint(t.accent)
            .labelsHidden()
            .accessibilityLabel("Show \(row.title) in Agents")
            .hookAction("settings:toggle:" + row.id) { set(row.id, shown: !row.shown) }
        }
        .frame(height: ShellSpace.settingsRowHeight)
        .contentShape(Rectangle())
    }
}

/// The row's agent face at settings size: the picture, else the initial on its tint (as AgentFace).
struct SettingsFace: View {
    let face: SpacesRow.Face
    let t: Tokens
    @ObservedObject private var pictures = FacePictures.shared

    var body: some View {
        Group {
            if let url = face.avatar, let image = pictures.images[url] {
                Image(nsImage: image).resizable().interpolation(.high).aspectRatio(contentMode: .fill)
                    .overlay(Circle().strokeBorder(t.line, lineWidth: ShellFace.ring))
            } else {
                let tint = Color(hex: ShellFace.tints[face.tint % ShellFace.tints.count])
                ZStack {
                    Circle().fill(tint.opacity(t.mode == .dark ? ShellFace.tintOpacityDark : ShellFace.tintOpacityLight))
                    Text(face.initial).font(.system(size: ShellFace.settingsInitial, weight: .semibold)).foregroundStyle(t.ink)
                }
            }
        }
        .frame(width: ShellFace.settingsSize, height: ShellFace.settingsSize)
        .clipShape(Circle())
        .task(id: face.avatar) { if let url = face.avatar { pictures.load(url) } }
    }
}
