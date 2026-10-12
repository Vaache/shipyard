// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "GitHubLite",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "GitHubLite", path: "Sources/GitHubLite")
    ]
)
