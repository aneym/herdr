// swift-tools-version:5.9
import Foundation
import PackageDescription

// The static libghostty (GhosttyKit) is built from vendor/ghostty with
// `zig build -Demit-xcframework=true -Dxcframework-target=native` and copied to
// Vendor/GhosttyKit by scripts/vendor-ghostty.sh.
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path

let package = Package(
    name: "HerdrShell",
    platforms: [.macOS(.v14)],
    targets: [
        .target(
            name: "GhosttyKit",
            path: "Sources/GhosttyKit",
            publicHeadersPath: "include"
        ),
        .executableTarget(
            name: "HerdrShell",
            dependencies: ["GhosttyKit"],
            path: "Sources/HerdrShell",
            linkerSettings: [
                .unsafeFlags(["-L\(root)/Vendor/GhosttyKit", "-lghostty-internal"]),
                .linkedLibrary("c++"),
                .linkedFramework("AppKit"),
                .linkedFramework("Carbon"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("CoreText"),
                .linkedFramework("CoreVideo"),
                .linkedFramework("IOSurface"),
                .linkedFramework("Metal"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("UniformTypeIdentifiers"),
                .linkedFramework("UserNotifications"),
                .linkedFramework("WebKit"),
            ]
        ),
    ]
)
