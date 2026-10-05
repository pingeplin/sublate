// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SublateKit",
    platforms: [.macOS(.v26)],
    products: [.library(name: "SublateUI", targets: ["SublateUI"])],
    targets: [
        .binaryTarget(name: "sublate_coreFFI", path: "sublate_coreFFI.xcframework"),
        .target(
            name: "SublateCore",
            dependencies: ["sublate_coreFFI"],
            linkerSettings: [
                .linkedFramework("Security"),
                .linkedFramework("CoreFoundation"),
                .linkedLibrary("iconv"),
            ]
        ),
        .target(name: "SublateUI", dependencies: ["SublateCore"]),
        .testTarget(name: "SublateUITests", dependencies: ["SublateUI"]),
    ]
)
