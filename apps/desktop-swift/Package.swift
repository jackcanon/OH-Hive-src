// swift-tools-version:5.10
import PackageDescription

// Terminal-driven build/run for the native macOS app (ADR-018), no Xcode project needed for
// day-to-day iteration -- `swift run` launches the real SwiftUI app window.
//
// Layout expected (see the setup commands Loki gave in chat):
//   Sources/ohhive_ffiFFI/module.modulemap, ohhive_ffiFFI.h   <- copied from crates/ohhive-ffi/bindings/
//   Sources/OHHiveFFI/ohhive_ffi.swift                        <- copied from crates/ohhive-ffi/bindings/
//   Sources/OHHive/*.swift                                    <- the app's own SwiftUI source
let package = Package(
    name: "OHHive",
    // macOS 27, Apple Silicon only (ADR-018 decision 8/amendment 2026-09-09) -- Foundation
    // Models' `LanguageModelSession`/`SystemLanguageModel`/`Tool`/`@Generable` need macOS 26+,
    // and this app's App Intents/WidgetKit work (task-72-adjacent) targets 27 specifically.
    // `.macOS("27.0")` (string form) is used instead of a `.v27` enum case since this toolchain's
    // PackageDescription library may predate that case being added.
    platforms: [.macOS("27.0")],
    targets: [
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
            name: "OHHive",
            dependencies: ["OHHiveFFI"],
            path: "Sources/OHHive",
            linkerSettings: [
                // Links the release build from `crates/ohhive-ffi` -- rebuild that first any
                // time the Rust side changes, this doesn't do it for you.
                .unsafeFlags(["-L../../target/aarch64-apple-darwin/release", "-lohhive_ffi"])
            ]
        ),
    ]
)
