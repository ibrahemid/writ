// swift-tools-version:5.9
// src-tauri/build.rs compiles Sources/JotraOnDevice with swiftc for each target
// it builds; this manifest lets the bridge be built and opened on its own.
import PackageDescription

let package = Package(
    name: "JotraOnDevice",
    platforms: [.macOS(.v12)],
    products: [
        .library(name: "JotraOnDevice", type: .static, targets: ["JotraOnDevice"])
    ],
    targets: [
        .target(name: "JotraOnDevice")
    ],
    swiftLanguageVersions: [.v5]
)
