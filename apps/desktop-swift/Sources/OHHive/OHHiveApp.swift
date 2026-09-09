import SwiftUI
import AppKit

/// `swift run` launches this as a bare executable, not a real `.app` bundle -- without this,
/// macOS doesn't reliably give the process regular/foreground app status, so `WindowGroup`
/// creates its window but never shows or focuses it (you only see the MenuBarExtra icon).
/// A real Xcode-built `.app` bundle wouldn't need this; keeping it is still harmless there.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
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
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(store)
                .frame(minWidth: 420, minHeight: 480)
        }
        .windowResizability(.contentSize)
        .commands {
            // Standard macOS convention: "About OH Hive" is its own menu item above Settings,
            // no shortcut -- replacing the default boring NSApplication About box with the
            // Happy Jack Media house-rule credit panel.
            CommandGroup(replacing: .appInfo) {
                Button("About OH Hive") { openWindow(id: "about") }
            }
        }

        // Standard macOS convention: Preferences/Settings lives under the app menu, `⌘,` --
        // not a sidebar tab. Parity with the Tauri app's Settings tab.
        Settings {
            SettingsView()
                .environmentObject(store)
        }

        Window("About OH Hive", id: "about") {
            AboutView()
                .environmentObject(store)
        }
        .windowResizability(.contentSize)

        MenuBarExtra("OH Hive", systemImage: "hexagon.fill") {
            MenuBarContent()
                .environmentObject(store)
        }
        .menuBarExtraStyle(.window)
    }
}
