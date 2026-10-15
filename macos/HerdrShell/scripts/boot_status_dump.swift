import Foundation

@main
struct BootStatusDump {
    static func main() {
        let input = FileHandle.standardInput.readDataToEndOfFile()
        let cases = (try? JSONSerialization.jsonObject(with: input)) as? [[String: Any]] ?? []
        let rows: [[String: Any]] = cases.map { c in
            let line = BootStatus.line(loaded: c["loaded"] as? Bool ?? false, error: c["error"] as? String,
                                       pendingRestores: c["pending"] as? Int ?? 0, waitedSeconds: c["waited"] as? Int ?? 0)
            guard let line else { return ["hidden": true] }
            return ["text": line.text, "centered": line.centered]
        }
        let data = try! JSONSerialization.data(withJSONObject: rows)
        FileHandle.standardOutput.write(data)
    }
}
