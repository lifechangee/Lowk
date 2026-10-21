// swift-tools-version:6.0
import PackageDescription
let package = Package(
    name: "applestt",
    platforms: [.macOS("26.0")],
    targets: [
        .executableTarget(
            name: "applestt",
            path: "Sources/applestt",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [.unsafeFlags(["-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist", "-Xlinker", "Info.plist"])]
        )
    ]
)
