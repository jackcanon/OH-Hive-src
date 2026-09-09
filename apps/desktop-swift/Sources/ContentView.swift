import SwiftUI

struct ContentView: View {
    @EnvironmentObject private var store: HiveStore

    var body: some View {
        NavigationSplitView {
            List {
                Label("Node", systemImage: "cpu")
            }
            .navigationTitle("OH Hive")
        } detail: {
            NodeView()
        }
        .onAppear { store.refresh() }
    }
}

/// Compact status + start/stop, for the menu bar extra. Phase 2 (ADR-018) adds a Server row
/// here once the regional-server role moves to Swift; today's Tauri app's tray-menu server
/// status has no equivalent in this phase-1 pass.
struct MenuBarContent: View {
    @EnvironmentObject private var store: HiveStore

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("OH Hive").font(.headline)
            if let snap = store.snapshot {
                Text(snap.paired ? (snap.running ? "Working" : "Paired, stopped") : "Not paired")
                    .foregroundStyle(.secondary)
                if snap.paired {
                    Button(snap.running ? "Stop working" : "Start working") {
                        Task {
                            if snap.running { await store.stopWorking() } else { await store.startWorking() }
                        }
                    }
                }
            } else {
                ProgressView()
            }
            Divider()
            Button("Quit") { NSApplication.shared.terminate(nil) }
        }
        .padding(12)
        .frame(width: 220)
    }
}
