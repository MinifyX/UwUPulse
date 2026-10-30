// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PulseKit",
    platforms: [.iOS(.v18), .watchOS(.v11), .macOS(.v15)],
    products: [
        .library(name: "PulseKit", targets: ["PulseKit"]),
    ],
    targets: [
        .target(name: "PulseKit"),
        .testTarget(name: "PulseKitTests", dependencies: ["PulseKit"]),
    ]
)
