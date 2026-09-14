import SwiftUI

enum Pane: String, CaseIterable, Identifiable {
    case dashboard = "Dashboard", run = "Run", history = "History", fleet = "Fleet", logs = "Logs"
    var id: String { rawValue }
    var icon: String {
        switch self {
        case .dashboard: "gauge.with.dots.needle.67percent"
        case .run: "play.circle"
        case .history: "doc.text.magnifyingglass"
        case .fleet: "server.rack"
        case .logs: "terminal"
        }
    }
}

struct ContentView: View {
    @Environment(BenchStore.self) private var store
    @State private var section: Pane = .dashboard

    var body: some View {
        NavigationSplitView {
            List(Pane.allCases, selection: $section) { s in
                Label(s.rawValue, systemImage: s.icon).tag(s)
            }
            .listStyle(.sidebar)
            .safeAreaInset(edge: .top) {
                HStack(spacing: 10) {
                    LogoView(size: 34)
                    VStack(alignment: .leading, spacing: 0) {
                        Text("HaloBench").font(.headline)
                        Text("Project Halo test bench").font(.caption2).foregroundStyle(.secondary)
                    }
                    Spacer()
                }
                .padding(.horizontal, 12).padding(.vertical, 10)
            }
            .safeAreaInset(edge: .bottom) { RunStatusPill().padding(10) }
            .navigationSplitViewColumnWidth(min: 190, ideal: 210)
        } detail: {
            switch section {
            case .dashboard: DashboardView(goToRun: { section = .run })
            case .run: RunView()
            case .history: HistoryView()
            case .fleet: FleetView()
            case .logs: LogView()
            }
        }
        .alert("HaloBench", isPresented: Binding(get: { store.errorMessage != nil }, set: { if !$0 { store.errorMessage = nil } })) {
            Button("OK") { store.errorMessage = nil }
        } message: { Text(store.errorMessage ?? "") }
    }
}

struct RunStatusPill: View {
    @Environment(BenchStore.self) private var store
    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { _ in
            VStack(alignment: .leading, spacing: 4) {
                switch store.phase {
                case .idle:
                    HStack(spacing: 8) { Circle().fill(.gray).frame(width: 8, height: 8); Text("Idle").font(.caption) }
                case .running:
                    HStack(spacing: 8) {
                        PulseDot(color: .honey)
                        Text("Running").font(.caption.bold()).foregroundStyle(Color.honey)
                        Spacer()
                        Text(RunTimelineView.fmt(store.timeline.elapsed)).font(.system(.caption, design: .monospaced))
                    }
                    Text(store.timeline.current?.title ?? "starting").font(.caption2).foregroundStyle(.secondary)
                    ProgressView(value: Double(store.timeline.current?.rawValue ?? 0), total: Double(Stage.allCases.count - 1))
                        .tint(.honey).controlSize(.small)
                case .finished(let o):
                    HStack(spacing: 8) { Image(systemName: o.symbol).foregroundStyle(o.color); Text("Last run: \(o.label)").font(.caption) }
                    if let f = store.timeline.failedStage { Text("failed at \(f.title)").font(.caption2).foregroundStyle(.secondary) }
                }
            }
            .padding(10)
            .background(store.phase == .running ? Color.honey.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
        }
    }
}

extension Outcome {
    var color: Color {
        switch self { case .pass: .green; case .fail: .red; case .partial: .orange; case .aborted: .gray }
    }
    var symbol: String {
        switch self { case .pass: "checkmark.circle.fill"; case .fail: "xmark.circle.fill"; case .partial: "exclamationmark.triangle.fill"; case .aborted: "stop.circle.fill" }
    }
}

extension Date {
    var short: String { formatted(date: .abbreviated, time: .shortened) }
}
