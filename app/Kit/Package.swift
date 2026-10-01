// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "ContentsTitleKit",
    platforms: [.macOS(.v26)],
    products: [.library(name: "ContentsTitleUI", targets: ["ContentsTitleUI"])],
    targets: [
        .binaryTarget(name: "contents_title_coreFFI", path: "contents_title_coreFFI.xcframework"),
        .target(
            name: "ContentsTitleCore",
            dependencies: ["contents_title_coreFFI"],
            linkerSettings: [
                .linkedFramework("Security"),
                .linkedFramework("CoreFoundation"),
                .linkedLibrary("iconv"),
            ]
        ),
        .target(name: "ContentsTitleUI", dependencies: ["ContentsTitleCore"]),
        .testTarget(name: "ContentsTitleUITests", dependencies: ["ContentsTitleUI"]),
    ]
)
