import SwiftUI

/// Sidebar.swift's click-target reporter, a no-op here: row_fit measures layout, not clicks.
extension View {
    func clickTarget(_ id: String) -> some View { self }
}
