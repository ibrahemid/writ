// swift-tools-version:5.9
// src-tauri/build.rs compiles Sources/WritOnDevice with swiftc for each target
// it builds; this manifest lets the bridge be built and opened on its own.
import PackageDescription

let package = Package(
    name: "WritOnDevice",
    platforms: [.macOS(.v12)],
    products: [
        .library(name: "WritOnDevice", type: .static, targets: ["WritOnDevice"])
    ],
    targets: [
        .target(name: "WritOnDevice")
    ],
    swiftLanguageVersions: [.v5]
)
