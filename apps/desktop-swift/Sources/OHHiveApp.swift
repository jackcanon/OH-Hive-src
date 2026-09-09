import SwiftUI

@main
struct OHHiveApp: App {
    @StateObject private var store = HiveStore()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(store)
                .frame(minWidth: 420, minHeight: 480)
        }
        .windowResizability(.contentSize)

        // Happy Jack Media house rule: About lives in Settings, not an ad-hoc window --
        // credits the maker and links the blog on every app built for this team.
        Settings {
            AboutView()
                .environmentObject(store)
        }

        MenuBarExtra("OH Hive", systemImage: "hexagon.fill") {
            MenuBarContent()
                .environmentObject(store)
        }
        .menuBarExtraStyle(.window)
    }
}
