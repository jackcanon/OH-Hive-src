import SwiftUI
import AppKit

/// `swift run` launches this as a bare executable, not a real `.app` bundle -- without this,
/// macOS doesn't reliably give the process regular/foreground app status, so `WindowGroup`
/// creates its window but never shows or focuses it (you only see the MenuBarExtra icon).
/// A real Xcode-built `.app` bundle wouldn't need this; keeping it is still harmless there.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
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

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(store)
                .environmentObject(google)
                .environmentObject(github)
                .frame(minWidth: 420, minHeight: 480)
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
