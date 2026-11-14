// swift-tools-version:5.3
import PackageDescription

let package = Package(
    name: "tauri-plugin-chart-picker",
    platforms: [.iOS(.v14)],
    products: [
        .library(name: "tauri-plugin-chart-picker", type: .static,
                 targets: ["tauri-plugin-chart-picker"])
    ],
    dependencies: [.package(name: "Tauri", path: "../.tauri/tauri-api")],
    targets: [
        .target(name: "tauri-plugin-chart-picker",
                dependencies: [.byName(name: "Tauri")], path: "Sources")
    ]
)
