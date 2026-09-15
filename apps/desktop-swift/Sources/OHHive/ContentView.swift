import SwiftUI
import OHHiveFFI

/// 2026-09-13, Jack's Cowork-style redesign (#191/#197): "I do want it to function very similarly
/// to CoWork. I want people to intuitively know where to look for things, so we can hide most of
/// the things in settings, and make sure we have Projects along the left side, and new chats etc."
/// This replaces the old flat `SidebarItem` list (Node/Server/Earnings/Kanban/Private Fleet/Chat/
/// Transcribe/Generate/Feedback all as equal-weight top-level tabs) with a "+ New chat" action, a
/// list of saved chats, and two clearly separate project sections -- everything else that used to
/// be a sidebar tab (Node, Server, Earnings, Transcribe, Generate, Feedback) got moved into
/// `SettingsView`'s tabs, per "hide most of the things in settings".
///
/// Same day, follow-up (Jack): "Hive and Private Fleet should be two tabs that differentiate
/// everything. So the Kanban for Private Fleet should be in a separate section than the Hive
/// Kanban. That way it really emphasizes the separation of Hive vs Private Fleet. We are working
/// towards a bring your own community concept, where Hive isn't specific to just one group, but it
/// can be re-used by other communities." -- so "Projects" is no longer one row: it's a "Hive"
/// section (the community Hive's own cloud projects, `HiveProjectsView`) and a "Private Fleet"
/// section (this machine's own local idea board, `PrivateFleetBoardView`, plus the existing
/// fleet-activity feed, `PrivateFleetView`) as two visually distinct groups. Private Fleet moved
/// back out of Settings to be a first-class section here, since it's no longer a minor tab but half
/// of the app's core split -- see also Jack's "folks who just want to run their own private fleet,
/// and don't connect to a hive" note: neither section depends on the other, so that already works
/// (`HiveProjectsView` just shows its pairing-required empty state; nothing here requires it).
/// Setup keeps its old special-cased behavior: while `!setupDone`, it's the only thing shown.
private enum SidebarSelection: Hashable {
    case setup
    case chat(UUID)
    case hiveProjects
    case privateFleetBoard
    case privateFleetActivity
    case privateFleetVault
    case settings
}

struct ContentView: View {
    @EnvironmentObject private var store: HiveStore
    @StateObject private var chatSessions = ChatSessionStore()
    @State private var selection: SidebarSelection?
    // Release notes (#178): fetched once per launch, as soon as the store has a paired snapshot
    // -- `checkedReleaseNotes` guards against re-checking on every later `snapshot` publish (the
    // 5s poll in HiveStore.refresh() republishes it repeatedly). An empty/error result just means
    // nothing to show; this is a nice-to-have; it should never block opening the app.
    @State private var releaseNotes: [ReleaseNote] = []
    @State private var checkedReleaseNotes = false
    // Update check (2026-09-13, Jack: "how do folks know that they need to update?" -- see
    // UpdateChecker.swift's doc for why this exists and why it's independent of pairing/the hub).
    // Checked once per launch regardless of pairing state. Originally a top-of-window banner via
    // `.safeAreaInset` -- 2026-09-13 follow-up, Jack: "The notification of a new version is not
    // in a good place... so it's not so fugly" -- that banner collided with the sidebar's/detail's
    // own title-bar text (NavigationSplitView doesn't reserve space for a container-level
    // safeAreaInset the way a single-pane view does; it renders as a stray overlay instead of
    // pushing each column's content down). Replaced with a quiet red dot on the sidebar's
    // "Settings" row (below) plus the real details inside `SettingsView`'s General tab -- no more
    // fighting the title bar, and it fits "hide most of the things in settings" besides.
    @State private var availableUpdate: UpdateChecker.AvailableUpdate?

    private var setupDone: Bool { store.snapshot?.setupDone ?? true }

    /// Mirrors the old `visibleItems`' setup gating: once setup is done, `.setup` is never a
    /// legal selection to land on or stay on.
    private var effectiveSelection: SidebarSelection {
        if !setupDone { return .setup }
        if let selection, selection != .setup { return selection }
        return .hiveProjects
    }

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            switch effectiveSelection {
            case .setup: SetupView()
            case .chat(let id): ChatView(sessionId: id)
            case .hiveProjects: HiveProjectsView()
            case .privateFleetBoard: PrivateFleetBoardView()
            case .privateFleetActivity: PrivateFleetView()
            case .privateFleetVault: VaultView()
            case .settings: SettingsView(availableUpdate: availableUpdate)
            }
        }
        .environmentObject(chatSessions)
        // 2026-09-13, Jack: "it should be a separate swift only app feature and it should be on
        // every screen of the swift app so that you can call it from everywhere. we'll keep it
        // everpresent until we hit a 1.0 release." An overlay at this top level, above the
        // NavigationSplitView, floats over Setup/every sidebar destination alike -- see
        // FeedbackAssistant.swift's header doc for why it's a wholly separate feature from
        // `ChatEngine`/`ChatView`, and `HiveVersion.isPre1_0` for how it retires itself at 1.0.
        .overlay(alignment: .bottomTrailing) {
            if HiveVersion.isPre1_0(store.about.appVersion) {
                FeedbackAssistantButton()
                    .padding(20)
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

    private var sidebar: some View {
        VStack(spacing: 0) {
            if setupDone {
                Button(action: startNewChat) {
                    Label("New chat", systemImage: "square.and.pencil")
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                Divider()
            }

            List(selection: Binding(
                get: { selection },
                set: { selection = $0 }
            )) {
                if setupDone {
                    Section("Chats") {
                        if chatSessions.sessions.isEmpty {
                            Text("No chats yet").font(.caption).foregroundStyle(.secondary)
                        }
                        ForEach(chatSessions.sessions) { session in
                            Label(session.title, systemImage: "bubble.left.and.bubble.right")
                                .lineLimit(1)
                                .tag(SidebarSelection.chat(session.id))
                                .contextMenu {
                                    Button("Delete", role: .destructive) {
                                        deleteSession(session)
                                    }
                                }
                        }
                    }
                    Section("Hive") {
                        Label("Projects", systemImage: "square.grid.3x3")
                            .tag(SidebarSelection.hiveProjects)
                    }
                    Section("Private Fleet") {
                        Label("Projects", systemImage: "checklist")
                            .tag(SidebarSelection.privateFleetBoard)
                        Label("Activity", systemImage: "antenna.radiowaves.left.and.right")
                            .tag(SidebarSelection.privateFleetActivity)
                        Label("Vault", systemImage: "books.vertical")
                            .tag(SidebarSelection.privateFleetVault)
                    }
                } else {
                    Label("Setup", systemImage: "wand.and.stars").tag(SidebarSelection.setup)
                }
            }
            .frame(maxHeight: .infinity)

            if setupDone {
                Divider()
                Button {
                    selection = .settings
                } label: {
                    HStack {
                        Label("Settings", systemImage: "gearshape")
                        if availableUpdate != nil {
                            // Quiet update indicator (2026-09-13, replacing the old top-of-window
                            // banner) -- a dot, not a badge with the version number, so it doesn't
                            // compete with the row's own label; the real "vX.Y.Z available,
                            // Download" text lives in SettingsView's General tab, one click away.
                            Circle().fill(Color.red).frame(width: 7, height: 7)
                        }
                        Spacer()
                    }
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
            }
        }
        // The `.navigationTitle` for a NavigationSplitView's sidebar column has to sit on the
        // OUTERMOST view returned for that column -- 2026-09-13 bug (Jack: "Somethings didn't
        // quite land"): this used to be on the inner `List` directly, back when the List *was*
        // the sidebar's entire content. Once the New Chat button/Divider/Settings button wrapped
        // it in this VStack, a title modifier left on the inner List no longer registers as the
        // column's title -- it renders as a stray overlay that collided with the detail pane's
        // own title ("Hive" and "Setup" overlapping in the title bar), and the List no longer
        // being the direct child NavigationSplitView expected also left it free to hug its
        // content height instead of filling the column (an empty-looking sidebar during Setup,
        // when only one row is shown) -- the `.frame(maxHeight: .infinity)` above is the other
        // half of that fix.
        .navigationTitle("Hive")
    }

    /// Reuses an already-open, still-empty draft chat instead of piling up blank "New chat"
    /// entries every time someone clicks the button without typing anything.
    private func startNewChat() {
        if let draft = chatSessions.sessions.first(where: { $0.messages.isEmpty }) {
            selection = .chat(draft.id)
        } else {
            let created = chatSessions.createSession()
            selection = .chat(created.id)
        }
    }

    private func deleteSession(_ session: ChatSession) {
        if case .chat(session.id) = effectiveSelection {
            selection = .hiveProjects
        }
        chatSessions.remove(session)
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
