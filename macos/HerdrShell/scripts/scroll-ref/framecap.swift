// framecap OWNER bounds            -> "x y w h" of OWNER's largest on-screen window (points)
// framecap OWNER SECONDS OUT.tsv [X Y W H] -> every frame the window server composites for that window
//   (ScreenCaptureKit drops unchanged frames), "ms<TAB>first line number on screen" per frame,
//   ms from the first frame. The number is read with Vision from the text "line N".
import AppKit
import CoreMedia
import Foundation
import ScreenCaptureKit
import Vision

let args = CommandLine.arguments
let owner = args[1]

func window() async throws -> SCWindow {
    let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
    let wins = content.windows.filter { $0.owningApplication?.applicationName == owner && $0.frame.height > 100 }
    guard let w = wins.max(by: { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }) else {
        fputs("no window for \(owner): \(content.windows.compactMap { $0.owningApplication?.applicationName })\n", stderr); exit(2)
    }
    return w
}

final class Sink: NSObject, SCStreamOutput {
    var frames: [(Double, CGImage)] = []
    let ctx = CIContext()
    func stream(_ stream: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen,
              let att = (CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]])?.first,
              let raw = att[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete,
              let pb = CMSampleBufferGetImageBuffer(sb) else { return }
        let t = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sb)) * 1000
        let ci = CIImage(cvPixelBuffer: pb)
        if let cg = ctx.createCGImage(ci, from: ci.extent) { frames.append((t, cg)) }
    }
}

let crop: CGRect? = args.count >= 8 ? CGRect(x: Double(args[4])!, y: Double(args[5])!, width: Double(args[6])!, height: Double(args[7])!) : nil
let octx = CIContext()

/// The text region (window pixels, top-left origin), upscaled 2x: Vision misses small light text.
func prepared(_ img: CGImage) -> CGImage {
    guard let crop, let c = img.cropping(to: crop) else { return img }
    let ci = CIImage(cgImage: c).transformed(by: CGAffineTransform(scaleX: 2, y: 2))
    return octx.createCGImage(ci, from: ci.extent) ?? c
}

func firstLine(_ raw: CGImage) -> Int? {
    let img = prepared(raw)
    let req = VNRecognizeTextRequest()
    req.recognitionLevel = .accurate
    req.usesLanguageCorrection = false
    try? VNImageRequestHandler(cgImage: img).perform([req])
    // Vision's y runs bottom-up. Every "line N" hit votes for the first visible row:
    // N minus its row index from the crop top, the row pitch taken from the hits themselves,
    // so one unread row does not lose the frame.
    // One observation can hold several rows, so take every match with its own box.
    var hits: [(CGFloat, Int)] = []
    for o in req.results ?? [] {
        guard let c = o.topCandidates(1).first else { continue }
        let s = c.string
        var from = s.startIndex
        while let m = s.range(of: #"line\s*(\d+)"#, options: .regularExpression, range: from..<s.endIndex) {
            from = m.upperBound
            guard let n = Int(s[m].filter(\.isNumber)) else { continue }
            let y = (try? c.boundingBox(for: m))??.boundingBox.midY ?? o.boundingBox.midY
            hits.append((y, n))
        }
    }
    hits.sort { $0.0 > $1.0 }
    guard hits.count >= 3 else { return nil }
    var pitches: [CGFloat] = []
    for i in 1..<hits.count where hits[i].1 > hits[i - 1].1 {
        pitches.append((hits[i - 1].0 - hits[i].0) / CGFloat(hits[i].1 - hits[i - 1].1))
    }
    guard !pitches.isEmpty else { return nil }
    let pitch = pitches.sorted()[pitches.count / 2]
    var votes: [Int: Int] = [:]
    for (y, n) in hits { votes[n - Int(((1 - y) / pitch).rounded(.down)), default: 0] += 1 }
    guard let best = votes.max(by: { $0.value < $1.value }), best.value * 2 > hits.count else { return nil }
    return best.key
}

let sem = DispatchSemaphore(value: 0)
Task {
    do {
        let w = try await window()
        if args[2] == "bounds" {
            print("\(Int(w.frame.minX)) \(Int(w.frame.minY)) \(Int(w.frame.width)) \(Int(w.frame.height))")
            exit(0)
        }
        let seconds = Double(args[2])!, out = args[3]
        let cfg = SCStreamConfiguration()
        cfg.width = Int(w.frame.width); cfg.height = Int(w.frame.height)
        cfg.minimumFrameInterval = CMTime(value: 1, timescale: 240)
        cfg.queueDepth = 8
        cfg.showsCursor = false
        let sink = Sink()
        let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: w), configuration: cfg, delegate: nil)
        try stream.addStreamOutput(sink, type: .screen, sampleHandlerQueue: DispatchQueue(label: "cap"))
        try await stream.startCapture()
        print("capturing"); fflush(stdout)
        try await Task.sleep(nanoseconds: UInt64(seconds * 1e9))
        try await stream.stopCapture()
        let t0 = sink.frames.first?.0 ?? 0
        var lines = "# frames=\(sink.frames.count) window=\(Int(w.frame.width))x\(Int(w.frame.height))\n"
        for (t, img) in sink.frames {
            let n = firstLine(img)
            lines += String(format: "%.1f\t%@\n", t - t0, n.map(String.init) ?? "?")
            if n == nil, ProcessInfo.processInfo.environment["FRAMECAP_DUMP"] != nil,
               let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out + String(format: ".%.0f.png", t - t0)) as CFURL, "public.png" as CFString, 1, nil) {
                CGImageDestinationAddImage(dest, prepared(img), nil); CGImageDestinationFinalize(dest)
            }
        }
        try lines.write(toFile: out, atomically: true, encoding: .utf8)
        if let img = sink.frames.last?.1,
           let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out + ".png") as CFURL, "public.png" as CFString, 1, nil) {
            CGImageDestinationAddImage(dest, img, nil); CGImageDestinationFinalize(dest)
        }
        print("wrote \(sink.frames.count) frames")
    } catch { fputs("framecap: \(error)\n", stderr); exit(1) }
    sem.signal()
}
sem.wait()
