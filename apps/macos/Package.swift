// swift-tools-version:6.2
import Foundation
import PackageDescription

let rustLib = ProcessInfo.processInfo.environment["PHOTOSINK_RUST_LIB"] ?? "../../target/release"

let package = Package(
    name: "Photosink",
    platforms: [.macOS(.v26)],
    targets: [
        .target(
            name: "photosink_bridgeFFI",
            path: "Sources/photosink_bridgeFFI"
        ),
        .executableTarget(
            name: "Photosink",
            dependencies: ["photosink_bridgeFFI"],
            path: "Sources/Photosink",
            linkerSettings: [
                .unsafeFlags(["-L\(rustLib)"]),
                .linkedLibrary("photosink_bridge"),
                .linkedFramework("Photos"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreFoundation"),
            ]
        ),
    ],
    swiftLanguageModes: [.v5]
)
