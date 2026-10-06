import AppKit

/// The existing server API validates PNG bytes and stages a private file on the
/// agent's host. Never substitute a client-local path when the upload fails.
enum ClipboardImagePaste {
    static let maxBytes = 16 * 1024 * 1024
    /// The pasteboard a terminal paste reads. Agent-run checks point it at a private
    /// named board so they never touch the user's clipboard.
    static var board: NSPasteboard = .general
    /// True only inside a user paste (Edit > Paste, Cmd-V). A program's OSC 52 read
    /// goes through the same Ghostty callback and must never upload an image.
    static var userPaste = false

    static func png(from pasteboard: NSPasteboard) -> Data? {
        if let data = pasteboard.data(forType: .png) { return data }
        guard let data = pasteboard.data(forType: .tiff),
              let bitmap = NSBitmapImageRep(data: data) else { return nil }
        return bitmap.representation(using: .png, properties: [:])
    }

    /// Blocking; callers run this off the main thread and retain the paste target.
    static func upload(_ png: Data, socketPath: String) -> String? {
        guard !socketPath.isEmpty, !png.isEmpty, png.count <= maxBytes else { return nil }
        let body: [String: Any] = [
            "id": "shell:clipboard-image", "method": "clipboard.image.write",
            "params": ["extension": "png", "data_base64": png.base64EncodedString()]
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let json = String(data: data, encoding: .utf8),
              let reply = HerdrSocket.request(socketPath, json, timeout: 30),
              let obj = try? JSONSerialization.jsonObject(with: reply) as? [String: Any],
              obj["error"] == nil,
              let result = obj["result"] as? [String: Any],
              let text = result["paste_text"] as? String, !text.isEmpty else { return nil }
        return text
    }
}
