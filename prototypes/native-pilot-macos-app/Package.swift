// swift-tools-version:5.10
import PackageDescription

// Second slice of ADR-029 gate #1's signing/TCC question. The first slice
// (../native-pilot-macos) proved a bare CLI binary launched from Terminal gets NO identity of
// its own -- the OS attributed the permission entirely to Terminal (confirmed live, see
// CONTINUITY.md). This package is the other half of that comparison: a real, LaunchServices
// -registered `.app` bundle, ad-hoc signed, launched with `open` (not run directly from a shell)
// -- does THAT get its own distinct entry in System Settings > Accessibility?
//
// build_app.sh in this directory does the bundle assembly + signing; `swift build` alone here
// only produces the bare executable that script then wraps.
let package = Package(
    name: "NativePilotApp",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "NativePilotApp",
            path: "Sources/NativePilotApp"
        )
    ]
)
