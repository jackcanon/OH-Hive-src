// swift-tools-version:5.10
import PackageDescription

// Hive Mobile (ADR-021) -- the iOS companion app. Unlike the macOS app (apps/desktop-swift),
// this one does NOT link ohhive-ffi/the Rust core at all: every feature in scope (Kanban, project
// creation, agent chat, forum, wallet, remote node checkout) is a *member*-authenticated action,
// which is just a Supabase RPC call -- the same thing the web app does with supabase-js, done here
// with the official supabase-swift client instead. This sidesteps the iOS cross-compile/xcframework
// problem entirely for v1 scope; see ADR-021 §5's note on why that packaging risk turned out not to
// apply here after all.
//
// IMPORTANT: this Package.swift alone does not produce an installable/runnable iOS app -- SwiftPM
// executables aren't iOS app bundles. See docs/IPHONE-APP-SCAFFOLD.md for the exact steps to wrap
// this package's Sources in a real Xcode iOS App project (File > New > Project > iOS > App), which
// is a five-minute, one-time, Xcode-only step I can't do from here.
let package = Package(
    name: "HiveMobile",
    platforms: [.iOS("27.0")],
    products: [
        .library(name: "HiveMobile", targets: ["HiveMobile"]),
    ],
    dependencies: [
        .package(url: "https://github.com/supabase/supabase-swift", from: "2.20.0"),
        // Google Sign-In has no SwiftPM-friendly pure-Swift equivalent worth pinning sight unseen;
        // add "https://github.com/google/GoogleSignIn-iOS" (from: "7.1.0") as an Xcode-side SPM
        // dependency once the real Xcode project exists -- see docs/IPHONE-APP-SCAFFOLD.md.
    ],
    targets: [
        .target(
            name: "HiveMobile",
            dependencies: [
                .product(name: "Supabase", package: "supabase-swift"),
            ],
            path: "Sources/OHHiveMobile"
        ),
    ]
)
