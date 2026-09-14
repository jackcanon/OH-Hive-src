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
    case privateFleet = "Private Fleet"
    case chat = "Chat"
    case transcribe = "Transcribe"
    case generate = "Generate"
    case feedback = "Feedback"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .setup: return "wand.and.stars"
        case .node: return "cpu"
        case .server: return "server.rack"
        case .earnings: return "chart.bar.fill"
        case .kanban: return "square.grid.3x3"
        case .privateFleet: return "lock.shield"
        case .chat: return "bubble.left.and.bubble.right"
        case .transcribe: return "waveform"
        case .generate: return "photo"
        case .feedback: return "lightbulb"
        }
    }
}

struct ContentView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var selection: SidebarItem?
    // Release notes (#178): fetched once per launch, as soon as the store has a paired snapshot
    // -- `checkedReleaseNotes` guards against re-checking on every later `snapshot` publish (the
    // 5s poll in HiveStore.refresh() republishes it repeatedly). An empty/error result just means
    // nothing to show; this is a nice-to-have; it should never block opening the app.
    @State private var releaseNotes: [ReleaseNote] = []
    @State private var checkedReleaseNotes = false
    // Update check (2026-09-13, Jack: "how do folks know that they need to update?" -- see
    // UpdateChecker.swift's doc for why this exists and why it's independent of pairing/the hub).
    // Checked once per launch regardless of pairing state; a slim dismissible banner, not a
    // `.sheet` like release notes -- an available update shouldn't block using the app.
    @State private var availableUpdate: UpdateChecker.AvailableUpdate?
    @State private var updateBannerDismissed = false

    /// Mirrors Tauri's `visibleTabs`/`cur ?? (setup_done ? "Node" : "Setup")`: hide Setup once
    /// first-run is done, and default the selection based on that same flag.
    private var visibleItems: [SidebarItem] {
        let done = store.snapshot?.setupDone ?? true
        return done ? [.node, .server, .earnings, .kanban, .privateFleet, .chat, .transcribe, .generate, .feedback] : SidebarItem.allCases
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
            case .privateFleet: PrivateFleetView()
            case .chat: ChatView()
            case .transcribe: TranscribeView()
            case .generate: GenerateImageView()
            case .feedback: FeedbackView()
            }
        }
        .safeAreaInset(edge: .top, spacing: 0) {
            if let update = availableUpdate, !updateBannerDismissed {
                UpdateBanner(update: update) { updateBannerDismissed = true }
            }
        }
        .onAppear {
            store.refresh()
            Task {
                availableUpdate = await UpdateChecker.check(currentVersion: store.about.appVersion)
            }
        }
        .onChange(of: store.snapshot?.paired) { _, paired in
            guard paired == true, !checkedReleaseNotes else { return }
            checkedReleaseNotes = true
            Task {
                if let unseen = await store.releaseNotesUnseen(), !unseen.isEmpty {
                    releaseNotes = unseen
                }
            }
        }
        .sheet(isPresented: Binding(
            get: { !releaseNotes.isEmpty },
            set: { if !$0 { releaseNotes = [] } }
        )) {
            ReleaseNotesView(notes: releaseNotes) {
                await store.releaseNotesMarkSeen()
                releaseNotes = []
            }
        }
    }
}

/// Slim top banner for an available update -- see `ContentView`'s `availableUpdate` doc comment
/// for why this exists and why it's dismissible rather than modal.
private struct UpdateBanner: View {
    let update: UpdateChecker.AvailableUpdate
    let onDismiss: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "arrow.down.circle.fill").foregroundStyle(.blue)
            Text("Hive v\(update.version) is available.")
            Link("Download", destination: update.url).fontWeight(.semibold)
            Spacer()
            Button { onDismiss() } label: { Image(systemName: "xmark") }
                .buttonStyle(.plain)
        }
        .font(.callout)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Color.blue.opacity(0.12))
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
