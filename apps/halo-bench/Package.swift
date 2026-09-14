// swift-tools-version:5.10
import PackageDescription

// HaloBench -- the Project Halo test bench (ADR-012 decision 17). A native macOS app that runs
// llama.cpp RPC split benchmarks across Loki's Lab, shows live status/logs, and files a
// human-readable report for every run into `docs/halo-reports/` (title + date/time), which is
// also what the History tab reads back -- the repo folder is the source of truth, the app is a
// mirror of it.
//
// No external dependencies. `swift run` launches the window; `scripts/build-app.sh` makes a
// real HaloBench.app with an icon.
let package = Package(
    name: "HaloBench",
    platforms: [.macOS("27.0")],
    targets: [
        .executableTarget(
            name: "HaloBench",
            path: "Sources/HaloBench"
        ),
    ]
)
