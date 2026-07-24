// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "AetherForge",
    platforms: [
        .macOS(.v15)
    ],
    products: [
        .library(
            name: "AetherForgeCore",
            targets: ["AetherForgeCore"]
        )
    ],
    targets: [
        .target(
            name: "AetherForgeCore",
            path: "macos/AetherForgeApp"
        )
    ]
)
