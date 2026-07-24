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
        .executableTarget(
            name: "AetherForgeApp",
            path: "macos/AetherForgeApp"
        )
    ]
)
