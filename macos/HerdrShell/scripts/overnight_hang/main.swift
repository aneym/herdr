import AppKit
import Darwin

// Driver for scripts/check_overnight_hang.py. Real libghostty, the shell's real config merge, one
// real surface fed by a subprocess flooding it with output, and the main-thread calls a
// display sleep/wake makes (content scale, display id, occlusion, size), repeated many
// times, then an idle stretch and a last wake. Prints one JSON line.
//
//   overnight_hang_driver <base.conf> <user.conf> <lines> <cycles> <idle-seconds>

let args = CommandLine.arguments
guard args.count == 6 else { FileHandle.standardError.write("usage: driver base user lines cycles idle\n".data(using: .utf8)!); exit(2) }
let base = try! String(contentsOfFile: args[1], encoding: .utf8)
let user = try! String(contentsOfFile: args[2], encoding: .utf8)
let lines = Int(args[3])!, cycles = Int(args[4])!, idle = Double(args[5])!

/// Threads in this process whose pthread name contains `needle` (CoreVideo names its
/// display-link IO threads "CVDisplayLink").
func threadsNamed(_ needle: String) -> Int {
    var list: thread_act_array_t?
    var count: mach_msg_type_number_t = 0
    guard task_threads(mach_task_self_, &list, &count) == KERN_SUCCESS, let list else { return -1 }
    defer {
        for i in 0..<Int(count) { mach_port_deallocate(mach_task_self_, list[i]) }
        vm_deallocate(mach_task_self_, vm_address_t(bitPattern: list), vm_size_t(Int(count) * MemoryLayout<thread_t>.stride))
    }
    var n = 0
    for i in 0..<Int(count) {
        var info = thread_extended_info_data_t()
        var size = mach_msg_type_number_t(MemoryLayout<thread_extended_info_data_t>.size / MemoryLayout<integer_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(size)) { thread_info(list[i], thread_flavor_t(THREAD_EXTENDED_INFO), $0, &size) }
        }
        guard kr == KERN_SUCCESS else { continue }
        let name = withUnsafeBytes(of: info.pth_name) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
        if name.contains(needle) { n += 1 }
    }
    return n
}

func ms(_ t0: UInt64) -> Double { Double(clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - t0) / 1e6 }
func now() -> UInt64 { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }

var argv0: [UnsafeMutablePointer<CChar>?] = [strdup(args[0]), nil]
guard ghostty_init(1, &argv0) == GHOSTTY_SUCCESS else { print("{\"error\":\"ghostty_init\"}"); exit(1) }

let nsapp = NSApplication.shared
nsapp.setActivationPolicy(.prohibited)

let text = GhosttyConfigMerge.merged(base: base, user: user)
let cfg = ghostty_config_new()!
let tmp = NSTemporaryDirectory() + "overnight-hang-\(getpid()).conf"
FileManager.default.createFile(atPath: tmp, contents: Data(text.utf8), attributes: [.posixPermissions: 0o600])
ghostty_config_load_file(cfg, tmp)
try? FileManager.default.removeItem(atPath: tmp)
ghostty_config_finalize(cfg)
var vsync = true
let key = "window-vsync"
_ = ghostty_config_get(cfg, &vsync, key, UInt(key.utf8.count))

var ghosttyApp: ghostty_app_t? = nil
var rt = ghostty_runtime_config_s(
    userdata: nil, supports_selection_clipboard: false,
    wakeup_cb: { _ in DispatchQueue.main.async { if let a = ghosttyApp { ghostty_app_tick(a) } } },
    action_cb: { _, _, _ in false },
    read_clipboard_cb: { _, _, _, _, _, _ in GHOSTTY_CLIPBOARD_READ_UNAVAILABLE },
    confirm_read_clipboard_cb: { _, _, _, _ in },
    write_clipboard_cb: { _, _, _, _, _ in },
    close_surface_cb: { _, _ in })
ghosttyApp = ghostty_app_new(&rt, cfg)
guard let app = ghosttyApp else { print("{\"error\":\"ghostty_app_new\"}"); exit(1) }

// Off screen and never ordered front: nothing appears on the desktop.
let window = NSWindow(contentRect: NSRect(x: -20000, y: -20000, width: 900, height: 600),
                      styleMask: [.borderless], backing: .buffered, defer: false)
let view = NSView(frame: NSRect(x: 0, y: 0, width: 900, height: 600))
view.wantsLayer = true
window.contentView = view

let flood = "yes 'merge-events relay lane-post bulletin 405 merges overnight sha 817c9d94 ok' | head -n \(lines); exec sleep 3600"
var scfg = ghostty_surface_config_new()
scfg.platform_tag = GHOSTTY_PLATFORM_MACOS
scfg.platform = ghostty_platform_u(macos: ghostty_platform_macos_s(nsview: Unmanaged.passUnretained(view).toOpaque()))
scfg.scale_factor = 2
scfg.wait_after_command = false
scfg.context = GHOSTTY_SURFACE_CONTEXT_SPLIT
let command = "/bin/sh -c \"\(flood)\""
let surface: ghostty_surface_t = command.withCString { c in
    "/tmp".withCString { cwd in
        scfg.command = c
        scfg.working_directory = cwd
        return ghostty_surface_new(app, &scfg)!
    }
}
ghostty_surface_set_content_scale(surface, 2, 2)
ghostty_surface_set_size(surface, 1800, 1200)
ghostty_surface_set_occlusion(surface, true)
ghostty_surface_set_focus(surface, true)

let displayId = CGMainDisplayID()
var maxCall = 0.0, maxGap = 0.0, maxLinkThreads = 0
var lastBeat = now()
let beat = Timer(timeInterval: 0.02, repeats: true) { _ in
    maxGap = max(maxGap, ms(lastBeat)); lastBeat = now()
    maxLinkThreads = max(maxLinkThreads, threadsNamed("CVDisplayLink"))
}
RunLoop.main.add(beat, forMode: .common)

func spin(_ s: Double) { RunLoop.main.run(until: Date(timeIntervalSinceNow: s)) }

// Let the flood land and the renderer settle into its steady state.
spin(3)

// What a night of display sleep/wake and reconfigure does to the main thread.
for i in 0..<cycles {
    let t0 = now()
    let scale = i % 2 == 0 ? 1.0 : 2.0
    ghostty_surface_set_occlusion(surface, false)
    ghostty_surface_set_content_scale(surface, scale, scale)
    ghostty_surface_set_display_id(surface, displayId)
    ghostty_surface_set_size(surface, UInt32(900 * scale), UInt32(600 * scale))
    ghostty_surface_set_occlusion(surface, true)
    ghostty_surface_refresh(surface)
    maxCall = max(maxCall, ms(t0))
    if i % 10 == 0 { spin(0.01) }
}
spin(1)

// Idle, then the wake that hung the app: one more backing-scale change on the main thread.
spin(idle)
lastBeat = now()
// The last cycle left scale 2 when `cycles` is even and 1 when odd; wake to the other one so
// libghostty does real work (a new DPI means a font grid change and a resize).
let wakeScale = cycles % 2 == 0 ? 1.0 : 2.0
let w0 = now()
ghostty_surface_set_content_scale(surface, wakeScale, wakeScale)
ghostty_surface_set_display_id(surface, displayId)
ghostty_surface_set_size(surface, UInt32(900 * wakeScale), UInt32(600 * wakeScale))
let wake = ms(w0)
spin(0.5)

let cols = ghostty_surface_size(surface).columns
print(String(format: "{\"vsync\":%@,\"display_link_threads\":%d,\"max_main_call_ms\":%.1f,\"max_heartbeat_gap_ms\":%.1f,\"wake_ms\":%.1f,\"cycles\":%d,\"lines\":%d,\"columns\":%d}",
             vsync ? "true" : "false", maxLinkThreads, maxCall, maxGap, wake, cycles, lines, Int(cols)))
exit(0)
