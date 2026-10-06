import Foundation

func log(_ s: String) {}

/// Reads ShellPaths from this process's env the way main does, then reloads the catalog once per
/// stdin line and prints the space groups it holds (`nil` when areas.json leaves the overlay's own).
@main struct LaneFilesDump {
    static func main() {
        let env = ProcessInfo.processInfo.environment
        ShellPaths.configure(env: env, home: env["HOME"] ?? NSHomeDirectory())
        print("areas " + ShellPaths.areas)
        fflush(stdout)
        let catalog = LaneCatalog()
        while readLine() != nil {
            catalog.reload()
            let groups = catalog.snapshot.spaceGroups.map { $0.map { $0.name + "=" + $0.spaces.joined(separator: ",") }.joined(separator: ";") }
            print("groups " + (groups ?? "nil"))
            fflush(stdout)
        }
    }
}
