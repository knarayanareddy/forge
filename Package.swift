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
        ),
        .library(
            name: "AetherForgeCore",
            targets: ["AetherForgeCore"]
        )
    ],
    targets: [
        .executableTarget(
            name: "AetherForgeApp",
            path: "macos/AetherForgeApp"
        ),
        .target(
            name: "AetherForgeCore",
            path: "macos/AetherForgeApp",
            sources: ["BookmarkManager.swift"]
        )
    ]
)
