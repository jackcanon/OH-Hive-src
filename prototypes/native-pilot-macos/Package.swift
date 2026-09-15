// swift-tools-version:5.10
import PackageDescription

// ADR-029 phase-2 acceptance gate #1 (design doc SIF-COMPUTER-USE-MACOS-DESIGN-2026-09-14.md,
// section 9): "signing/TCC/XPC proof, picker/window filtering, geometry and AX bounds, local
// indicator and Stop." This package is the smallest slice of that gate, not the whole thing --
// see README.md in this directory for exactly what it does and does not prove.
//
// Deliberately its own standalone package, not a target inside apps/desktop-swift -- this is a
// throwaway prototype for a permission/signing question, not production surface, and it should
// be trivial to delete once the question is answered.
let package = Package(
    name: "NativePilot",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "NativePilot",
            path: "Sources/NativePilot"
        )
    ]
)
