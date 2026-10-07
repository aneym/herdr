import Foundation

// Appended to the production sources by scripts/check_agents_hide_home.py and run through the Swift
// interpreter, so no new executable starts. Usage: <socket|-> <tunnels.json|-> <fixture.json>...
// Each fixture is a SpacesInput plus `remote`: other machines' raw session.snapshot replies, which
// take the production path (Machines.namespace, Snapshot, MachineRows.inputs, MachineMerge).
// For each fixture it prints "== <path>", the persisted chrome, the ⌘1..9 order, every row's dump,
// each row's hide menu and drag section, and the close order and reveal for `select`.
// With a socket it then sends tab.set_hidden three times: hide and show on the local server, then
// show for an ax42 tab, which tunnels.json routes to ax42's socket.

struct HideHomeFixture: Decodable {
    let input: SpacesInput; let overlay: Overlay; let chrome: SpacesChrome; let now: Double
    var toggles: [String]? = nil; var select: String? = nil; var selectSpace: String? = nil
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(2)
}

let args = Array(CommandLine.arguments.dropFirst())
guard args.count >= 2 else { fail("usage: <socket|-> <tunnels.json|-> <fixture.json>...") }
for path in args.dropFirst(2) {
    print("== " + path)
    guard let data = FileManager.default.contents(atPath: path) else { fail("unreadable " + path) }
    let fixture = try JSONDecoder().decode(HideHomeFixture.self, from: data)
    let raw = (try JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    let states: [MachineState] = try ((raw["remote"] as? [[String: Any]]) ?? []).map { remote in
        guard let name = remote["name"] as? String, let snapshot = remote["snapshot"] else { fail("remote needs name and snapshot") }
        let reply = Machines.namespace(try JSONSerialization.data(withJSONObject: snapshot), machine: name)
        return MachineState(name: name, snapshot: try JSONDecoder().decode(Snapshot.self, from: reply))
    }
    let machines = MachineRows.inputs(states)
    let input = MachineMerge.merge(fixture.input, machines: machines)
    var chrome = fixture.chrome
    for key in fixture.toggles ?? [] { chrome.toggle(key) }
    // The sidebar saves SpacesChrome as JSON; draw from what a relaunch would read back.
    let saved = try JSONDecoder().decode(SpacesChrome.self, from: JSONEncoder().encode(chrome))
    print("chrome|\(saved.hiddenAgentsExpanded)|\(saved.hiddenExpanded)")
    let rows = MachineMerge.badge(SpacesTree.build(input, overlay: fixture.overlay, chrome: saved, now: fixture.now), machines: machines)
    print("numbered|" + SpacesTree.numbered(input.tabs, rows: rows).joined(separator: ","))
    for row in rows { print(row.dump) }
    for row in rows {
        let menu = row.setsHidden.map { $0 ? "hide" : "show" } ?? ""
        print("menu|\(row.id)|\(menu)")
        print("drag|\(row.id)|\(SpacesTree.pinSection(of: row.id) ?? "")")
    }
    if let selected = fixture.select {
        print("close|" + SpacesTree.closePinOrder(rows, selected: selected).joined(separator: ","))
        var revealed = saved
        let changed = revealed.reveal(space: fixture.selectSpace ?? "", parked: false, selected: selected, rows: rows)
        print("reveal|\(changed)|" + revealed.collapsedSpaces.sorted().joined(separator: ","))
    }
}

if args[0] != "-" {
    Machines.configure(env: ["HERDR_SHELL_MACHINES": args[1]])
    let commands = HerdrCommands(socketPath: args[0])
    let hide = commands.tabSetHidden(tabId: "w1:t2", hidden: true)
    let unknown = commands.tabSetHidden(tabId: "w1:t2", hidden: false)
    let remote = commands.tabSetHidden(tabId: "ax42/w1:t2", hidden: false)
    print("transport|\(hide)|\(unknown)|\(remote)")
}
