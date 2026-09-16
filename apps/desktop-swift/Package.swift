// swift-tools-version:5.10
import PackageDescription
import Foundation

// Bundle builds link the exact library snapshot used to generate their bindings.
let ffiLibraryDirectory = ProcessInfo.processInfo.environment["OHHIVE_FFI_LIBRARY_DIR"]
    ?? "../../target/aarch64-apple-darwin/release"

// Terminal-driven build/run for the native macOS app (ADR-018), no Xcode project needed for
// day-to-day iteration -- `swift run` launches the real SwiftUI app window.
//
// Layout expected (see the setup commands Loki gave in chat):
//   Sources/ohhive_ffiFFI/module.modulemap, ohhive_ffiFFI.h   <- copied from crates/ohhive-ffi/bindings/
//   Sources/OHHiveFFI/ohhive_ffi.swift                        <- copied from crates/ohhive-ffi/bindings/
//   Sources/OHHive/*.swift                                    <- the app's own SwiftUI source
let package = Package(
    name: "Hive",
    // macOS 27, Apple Silicon only (ADR-018 decision 8/amendment 2026-09-09) -- Foundation
    // Models' `LanguageModelSession`/`SystemLanguageModel`/`Tool`/`@Generable` need macOS 26+,
    // and this app's App Intents/WidgetKit work (task-72-adjacent) targets 27 specifically.
    // `.macOS("27.0")` (string form) is used instead of a `.v27` enum case since this toolchain's
    // PackageDescription library may predate that case being added.
    platforms: [.macOS("27.0")],
    targets: [
        .testTarget(
            name: "HiveTests",
            dependencies: ["Hive", "OHHiveFFI"],
            linkerSettings: [.unsafeFlags(["-L\(ffiLibraryDirectory)", "-lohhive_ffi"])]
        ),
        // The C shim UniFFI generated. Module name must stay exactly `ohhive_ffiFFI` -- that's
        // what the generated `ohhive_ffi.swift` does `import ohhive_ffiFFI` for. No sources to
        // compile, just the modulemap + header, hence `systemLibrary` rather than `target`.
        .systemLibrary(
            name: "ohhive_ffiFFI",
            path: "Sources/ohhive_ffiFFI"
        ),
        // The real generated Swift API wrapping `crates/ohhive-ffi`'s `HiveNode`.
        .target(
            name: "OHHiveFFI",
            dependencies: ["ohhive_ffiFFI"],
            path: "Sources/OHHiveFFI"
        ),
        .executableTarget(
            name: "Hive",
            dependencies: ["OHHiveFFI"],
            path: "Sources/OHHive",
            linkerSettings: [
                // build-app.sh rebuilds Rust and supplies its isolated library snapshot.
                // Direct swift builds use the development library path by default.
                .unsafeFlags(["-L\(ffiLibraryDirectory)", "-lohhive_ffi"])
            ]
        ),
    ]
)
