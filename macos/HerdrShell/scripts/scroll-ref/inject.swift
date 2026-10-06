// inject PID X Y DY STEPS MOMENTUM DECAY INTERVAL_MS [hid]
// One trackpad swipe as AppKit delivers it: continuous pixel deltas with scroll phase
// began/changed/ended, then momentum events that decay. Same stream as TestHook.scrollGesture.
// Built and run inside the Cua Space (swiftc -O inject.swift -o inject). Pass `hid`: events
// posted to a pid never reached Ghostty.app's view; the HID tap at X,Y does. Ghostty.app ran
// wheellog.py as its command, so each wheel report it sent was logged.
import CoreGraphics
import Foundation
let a = CommandLine.arguments
let pid = pid_t(a[1])!, x = Double(a[2])!, y = Double(a[3])!, dy = Double(a[4])!
let steps = Int(a[5])!, momentum = Int(a[6])!, decay = Double(a[7])!, interval = Double(a[8])! / 1000
let hid = a.count > 9
var events: [(Int64, Int64, Double)] = [(1, 0, dy)]
events += Array(repeating: (2, 0, dy), count: max(0, steps - 2))
events.append((4, 0, 0))
for i in 0..<momentum {
    events.append((0, i == 0 ? 1 : (i == momentum - 1 ? 3 : 2), dy * pow(decay, Double(i + 1))))
}
if hid, let mv = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left) {
    mv.post(tap: .cghidEventTap)
    usleep(100_000)
}
let t0 = Date()
for (n, (phase, mom, d)) in events.enumerated() {
    let wait = t0.addingTimeInterval(Double(n) * interval).timeIntervalSinceNow
    if wait > 0 { usleep(useconds_t(wait * 1_000_000)) }
    guard let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1,
                           wheel1: Int32(d.rounded()), wheel2: 0, wheel3: 0) else { continue }
    cg.location = CGPoint(x: x, y: y)
    cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
    cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: phase)
    cg.setIntegerValueField(.scrollWheelEventMomentumPhase, value: mom)
    cg.setDoubleValueField(.scrollWheelEventPointDeltaAxis1, value: d)
    if hid { cg.post(tap: .cghidEventTap) } else { cg.postToPid(pid) }
}
print("posted \(events.count) events over \(Int(Date().timeIntervalSince(t0) * 1000)) ms")
