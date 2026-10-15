// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "PullYard",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/migueldeicaza/SwiftTerm", exact: "1.20.0")
    ],
    targets: [
        .executableTarget(
            name: "PullYard",
            dependencies: [.product(name: "SwiftTerm", package: "SwiftTerm")],
            path: "Sources/PullYard"
        )
    ]
)
