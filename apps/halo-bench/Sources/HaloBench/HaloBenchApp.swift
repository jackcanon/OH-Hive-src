import SwiftUI
import AppKit

@main
struct HaloBenchApp: App {
    @State private var store = BenchStore()

    var body: some Scene {
        WindowGroup("HaloBench") {
            ContentView()
                .environment(store)
                .frame(minWidth: 1260, minHeight: 720)
        }
        .commands {
            // Standard macOS "About HaloBench" shows the same content as Settings -> About.
            CommandGroup(replacing: .appInfo) {
                Button("About HaloBench") { AboutWindow.show() }
            }
            CommandGroup(after: .newItem) {
                Button("Reload History") { store.reload() }.keyboardShortcut("r", modifiers: .command)
                Button("Run Test") { store.start() }.keyboardShortcut(.return, modifiers: .command)
                    .disabled(store.phase == .running)
            }
        }

        Settings {
            SettingsView().environment(store)
        }
    }
}

/// Hosts the About view in its own small window for the app-menu item.
enum AboutWindow {
    private static var window: NSWindow?
    static func show() {
        if let w = window { w.makeKeyAndOrderFront(nil); return }
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 420, height: 520),
                         styleMask: [.titled, .closable], backing: .buffered, defer: false)
        w.title = "About HaloBench"
        let hv = NSHostingView(rootView: AboutView().frame(width: 420))
        hv.sizingOptions = []   // don't let the hosting view resize the window to an unbounded ideal height
        w.contentView = hv
        w.setContentSize(NSSize(width: 420, height: 520))
        w.isReleasedWhenClosed = false
        w.center()
        w.makeKeyAndOrderFront(nil)
        window = w
    }
}
