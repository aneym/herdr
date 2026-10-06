import AppKit

/// Integration regression: real AppKit image data crosses the JSON socket boundary.
/// It protects remote-host staging (the old Shell accepted only text), not API shape.
@main struct ClipboardImageCheck {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else { fatalError("expected API socket path") }
        let board = NSPasteboard.withUniqueName()
        defer { board.releaseGlobally() }
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 2, pixelsHigh: 2,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .calibratedRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        for y in 0..<2 { for x in 0..<2 { bitmap.setColor(.blue, atX: x, y: y) } }
        let png = bitmap.representation(using: .png, properties: [:])!
        board.setData(bitmap.tiffRepresentation!, forType: .tiff)
        guard let converted = ClipboardImagePaste.png(from: board),
              NSBitmapImageRep(data: converted)?.pixelsWide == 2 else {
            fatalError("TIFF clipboard did not become PNG")
        }
        board.clearContents()
        board.setData(png, forType: .png)
        guard ClipboardImagePaste.png(from: board) == png else { fatalError("PNG clipboard bytes changed") }
        guard let path = ClipboardImagePaste.upload(png, socketPath: CommandLine.arguments[1]) else {
            fatalError("server did not stage clipboard image")
        }
        print("PASS clipboard image uploaded; agent-host paste path: \(path)")
        guard ClipboardImagePaste.upload(Data(), socketPath: CommandLine.arguments[1]) == nil,
              ClipboardImagePaste.upload(png, socketPath: "/nonexistent/herdr.sock") == nil else {
            fatalError("failed upload produced a paste path")
        }
        print("PASS failures never paste a client-local path")
    }
}
