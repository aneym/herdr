import Foundation

// socket_deadline <socket> <payload-bytes> <timeout-seconds>
// Sends one request of about <payload-bytes> through HerdrSocket.request and prints
// "<elapsed seconds> <reply or nil>".

@main
struct SocketDeadline {
    static func main() {
        let a = CommandLine.arguments
        guard a.count == 4, let size = Int(a[2]), let timeout = Double(a[3]) else {
            FileHandle.standardError.write(Data("usage: socket_deadline <socket> <bytes> <timeout>\n".utf8))
            exit(2)
        }
        let json = #"{"id":"t","method":"m","params":{"data":""# + String(repeating: "A", count: size) + #""}}"#
        let t0 = Date()
        let reply = HerdrSocket.request(a[1], json, timeout: timeout)
        let text = reply.flatMap { String(data: $0, encoding: .utf8) } ?? "nil"
        print(String(format: "%.2f", Date().timeIntervalSince(t0)), text)
    }
}
