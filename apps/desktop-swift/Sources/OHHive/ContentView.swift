import SwiftUI
import OHHiveFFI

/// Sidebar sections -- workspace content only. Tauri's tab bar (`apps/desktop/src/App.tsx`'s
/// `TABS`) also lists Settings and About as tabs, but on the Mac those belong under the app
/// menu instead (`⌘,` for Settings, "About Hive" above it) -- see `OHHiveApp.swift`.
private enum SidebarItem: String, CaseIterable, Identifiable {
    case setup = "Setup"
    case node = "Node"
    case server = "Server"
    case earnings = "Earnings"
    case kanban = "Kanban"
    case chat = "Chat"
    case transcribe = "Transcribe"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .setup: return "wand.and.stars"
        case .node: return "cpu"
        case .server: return "server.rack"
        case .earnings: return "chart.bar.fill"
        case .kanban: return "square.grid.3x3"
        case .chat: return "bubble.left.and.bubble.right"
        case .transcribe: return "waveform"
        }
    }
}

struct ContentView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var selection: SidebarItem?

    /// Mirrors Tauri's `visibleTabs`/`cur ?? (setup_done ? "Node" : "Setup")`: hide Setup once
    /// first-run is done, and default the selection based on that same flag.
    private var visibleItems: [SidebarItem] {
        let done = store.snapshot?.setupDone ?? true
        return done ? [.node, .server, .earnings, .kanban, .chat, .transcribe] : SidebarItem.allCases
    }

    private var effectiveSelection: SidebarItem {
        if let selection, visibleItems.contains(selection) { return selection }
        return (store.snapshot?.setupDone ?? true) ? .node : .setup
    }

    var body: some View {
        NavigationSplitView {
            List(visibleItems, selection: $selection) { item in
                Label(item.rawValue, systemImage: item.icon).tag(item)
            }
            .navigationTitle("Hive")
        } detail: {
            switch effectiveSelection {
            case .setup: SetupView()
            case .node: NodeView()
            case .server: ServerView()
            case .earnings: EarningsView()
            case .kanban: KanbanView()
            case .chat: ChatView()
            case .transcribe: TranscribeView()
            }
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
            Text("Hive").font(.headline)
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
