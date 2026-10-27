import SwiftUI
import AppKit
import Darwin
import OHHiveFFI

@MainActor
final class AppTerminationCoordinator {
    private var waiting = false
    private(set) var readyToExit = false

    func request(shutdown: @escaping @MainActor () async -> Bool,
                 reply: @escaping @MainActor (Bool) -> Void) {
        guard !waiting else { return }
        waiting = true
        Task {
            let safeToExit = await shutdown()
            readyToExit = safeToExit
            waiting = false
            reply(safeToExit)
        }
    }
}

/// `swift run` launches this as a bare executable, not a real `.app` bundle -- without this,
/// macOS doesn't reliably give the process regular/foreground app status, so `WindowGroup`
/// creates its window but never shows or focuses it (you only see the MenuBarExtra icon).
/// A real Xcode-built `.app` bundle wouldn't need this; keeping it is still harmless there.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    var shutdown: (@MainActor () async -> Bool)?
    private let termination = AppTerminationCoordinator()
    private var terminationSignal: DispatchSourceSignal?

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if termination.readyToExit { return .terminateNow }
        guard let shutdown else { return .terminateNow }
        termination.request(shutdown: shutdown) { safeToExit in
            if safeToExit { sender.terminate(nil) }
        }
        // terminateLater enters Cocoa's nested termination loop, which can starve
        // the Swift main-actor task performing cleanup. Return to the normal loop
        // first; the completed cleanup reissues termination with readyToExit set.
        return .terminateCancel
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Route SIGTERM through the same cooperative shutdown as Cmd-Q. SIGKILL and
        // host failure still require an independent command guardian/isolation layer.
        // A caught disposition resets on exec; SIG_IGN would leak into tool children.
        signal(SIGTERM) { _ in }
        let source = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        source.setEventHandler { NSApplication.shared.terminate(nil) }
        terminationSignal = source
        source.resume()
        // The launcher execs Hive-bin; explicitly set the running Dock icon rather
        // than relying on LaunchServices' cached icon for the helper executable.
        if let url = Bundle.main.url(forResource: "AppIcon", withExtension: "icns"),
           let icon = NSImage(contentsOf: url) {
            NSApp.applicationIconImage = icon
        }
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        for window in NSApp.windows {
            window.makeKeyAndOrderFront(nil)
        }
    }
}

@main
struct OHHiveApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var store = HiveStore()
    @StateObject private var google = GoogleAuthManager()
    @StateObject private var github = GitHubAuthManager()
    @Environment(\.openWindow) private var openWindow

    init() {
        if CommandLine.arguments.contains("--hive-engine-smoke-check") {
            // Exercise the generated bindings' complete API checksum table, not just
            // dlopen or a source stamp. No scene, worker, network or vault is opened.
            let node = HiveNode()
            guard !node.about().coreVersion.isEmpty else { exit(1) }
            print("Hive engine API initialized successfully.")
            exit(0)
        }
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(store)
                .environmentObject(google)
                .environmentObject(github)
                .frame(minWidth: 420, minHeight: 480)
                .onAppear {
                    let worker = store.codingWorker
                    appDelegate.shutdown = { await worker.shutdown() }
                }
        }
        .defaultSize(width: 1100, height: 760)
        .windowResizability(.contentMinSize)
        .commands {
            // Standard macOS convention: "About Loki's Den" is its own menu item above Settings,
            // no shortcut -- replacing the default boring NSApplication About box with the
            // Happy Jack Media house-rule credit panel.
            CommandGroup(replacing: .appInfo) {
                Button("About Loki's Den") { openWindow(id: "about") }
            }
        }

        // Standard macOS convention: Preferences/Settings lives under the app menu, `⌘,` --
        // not a sidebar tab. Parity with the Tauri app's Settings tab.
        Settings {
            SettingsView()
                .environmentObject(store)
                .environmentObject(google)
                .environmentObject(github)
        }

        Window("About Loki's Den", id: "about") {
            AboutView()
                .environmentObject(store)
                .environmentObject(google)
                .environmentObject(github)
        }
        .windowResizability(.contentSize)

        MenuBarExtra("Loki's Den", systemImage: "door.left.hand.closed") {
            MenuBarContent()
                .environmentObject(store)
                .environmentObject(google)
                .environmentObject(github)
        }
        .menuBarExtraStyle(.window)
    }
}
