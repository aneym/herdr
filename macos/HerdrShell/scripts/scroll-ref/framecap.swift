// framecap X Y W H SECONDS OUT
// Presented frames of one screen rect (points, top-left origin) through ScreenCaptureKit,
// for SECONDS. ScreenCaptureKit hands over a frame each time the window server composites
// a changed one, so the stream is what the display showed. For each frame OUT gets
// "ms<TAB>shift_px<TAB>err<TAB>status": the vertical move of the content against the frame
// before it (best match of per-row luminance sums; content moving down is positive) and
// the match error per pixel (0 = an exact move). Idle reports (no change) are listed too.
// Built and run inside the Cua Space (swiftc -O framecap.swift -o framecap).
import CoreMedia
import Foundation
import ScreenCaptureKit

let a = CommandLine.arguments
let rect = CGRect(x: Double(a[1])!, y: Double(a[2])!, width: Double(a[3])!, height: Double(a[4])!)
let seconds = Double(a[5])!, out = a[6]

final class Sink: NSObject, SCStreamOutput {
    var frames: [(Double, [Double]?)] = []
    let lock = NSLock()
    func stream(_ stream: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen,
              let infos = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let raw = infos.first?[.status] as? Int, let status = SCFrameStatus(rawValue: raw) else { return }
        let t = CMSampleBufferGetPresentationTimeStamp(sb).seconds * 1000
        if status == .idle { lock.lock(); frames.append((t, nil)); lock.unlock(); return }
        guard status == .complete, let px = CMSampleBufferGetImageBuffer(sb) else { return }
        CVPixelBufferLockBaseAddress(px, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(px, .readOnly) }
        let w = CVPixelBufferGetWidth(px), h = CVPixelBufferGetHeight(px), stride = CVPixelBufferGetBytesPerRow(px)
        guard let base = CVPixelBufferGetBaseAddress(px)?.assumingMemoryBound(to: UInt8.self) else { return }
        var sig = [Double](repeating: 0, count: h)
        for y in 0..<h {
            let row = base + y * stride
            var s = 0
            for x in 0..<w { let p = row + x * 4; s += Int(p[0]) + Int(p[1]) * 2 + Int(p[2]) }
            sig[y] = Double(s) / Double(w * 4)
        }
        lock.lock(); frames.append((t, sig)); lock.unlock()
    }
}

func shift(_ a: [Double], _ b: [Double], limit: Int) -> (Int, Double) {
    var best = (0, Double.infinity)
    for s in -limit...limit {
        var err = 0.0, n = 0
        for y in max(0, s)..<min(b.count, a.count + s) { err += abs(b[y] - a[y - s]); n += 1 }
        if n >= a.count / 2, err / Double(n) < best.1 { best = (s, err / Double(n)) }
    }
    return best
}

let sink = Sink()
let sem = DispatchSemaphore(value: 0)
Task {
    do {
        let content = try await SCShareableContent.current
        guard let display = content.displays.first else { fatalError("no display") }
        let config = SCStreamConfiguration()
        config.sourceRect = rect
        config.width = Int(rect.width)
        config.height = Int(rect.height)
        config.minimumFrameInterval = CMTime(value: 1, timescale: 240)
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.showsCursor = false
        config.queueDepth = 8
        let stream = SCStream(filter: SCContentFilter(display: display, excludingWindows: []), configuration: config, delegate: nil)
        try stream.addStreamOutput(sink, type: .screen, sampleHandlerQueue: DispatchQueue(label: "cap"))
        try await stream.startCapture()
        print("capturing \(rect) for \(seconds)s")
        try await Task.sleep(nanoseconds: UInt64(seconds * 1e9))
        try await stream.stopCapture()
    } catch { print("framecap: \(error)"); exit(1) }
    sem.signal()
}
sem.wait()
sink.lock.lock()
let frames = sink.frames
sink.lock.unlock()
var lines = ["# frames=\(frames.filter { $0.1 != nil }.count) idle=\(frames.filter { $0.1 == nil }.count) rect=\(rect)"]
var prev: [Double]? = nil
let t0 = frames.first?.0 ?? 0
for (t, sig) in frames {
    guard let sig else { lines.append(String(format: "%.2f\t0\t0\tidle", t - t0)); continue }
    if let p = prev {
        let (s, e) = shift(p, sig, limit: sig.count / 2)
        lines.append(String(format: "%.2f\t%d\t%.3f\tframe", t - t0, s, e))
    } else {
        lines.append(String(format: "%.2f\t0\t0\tfirst", t - t0))
    }
    prev = sig
}
try! (lines.joined(separator: "\n") + "\n").write(toFile: out, atomically: true, encoding: .utf8)
print("wrote \(frames.count) frames to \(out)")
