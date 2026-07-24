// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "AetherForge",
    platforms: [
        .macOS(.v15)
    ],
    products: [
        .executable(
            name: "AetherForgeApp",
            targets: ["AetherForgeApp"]
        )
    ],
    targets: [
        .target(
            name: "AetherFFI",
            path: "macos/AetherFFI",
            publicHeadersPath: "include",
            linkerSettings: [
                .linkedLibrary("aether_ffi"),
                .unsafeFlags([
                    "-L", "target/debug",
                    "-L", "target/release"
                ], .when(platforms: [.macOS]))
            ]
        ),
        .executableTarget(
            name: "AetherForgeApp",
            dependencies: ["AetherFFI"],
            path: "macos/AetherForgeApp"
        )
    ]
)
