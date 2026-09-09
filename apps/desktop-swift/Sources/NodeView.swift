import SwiftUI

struct NodeView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var showPairSheet = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let snap = store.snapshot {
                    statusCard(snap)
                    if snap.paired {
                        workerCard(snap)
                    }
                    activityList()
                } else {
                    ProgressView("Loading\u{2026}")
                }
                if let err = store.lastError {
                    Text(err).foregroundStyle(.red).font(.callout)
                }
            }
            .padding(20)
        }
        .navigationTitle("Node")
        .sheet(isPresented: $showPairSheet) {
            PairView(isPresented: $showPairSheet)
        }
    }

    @ViewBuilder
    private func statusCard(_ snap: HiveSnapshot) -> some View {
        GroupBox("This machine") {
            VStack(alignment: .leading, spacing: 6) {
                if snap.paired {
                    Label("Paired", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                    Text(snap.hubUrl).font(.caption).foregroundStyle(.secondary)
                } else {
                    Label("Not paired", systemImage: "xmark.circle").foregroundStyle(.secondary)
                    Button("Pair this machine") { showPairSheet = true }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func workerCard(_ snap: HiveSnapshot) -> some View {
        GroupBox("Working") {
            VStack(alignment: .leading, spacing: 8) {
                Text(snap.backendOk
                     ? "\(snap.models.count) model(s) available"
                     : "No backend reachable at \(snap.llamaUrl)")
                    .font(.callout)
                HStack {
                    Button(snap.running ? "Stop working" : "Start working") {
                        Task {
                            if snap.running { await store.stopWorking() } else { await store.startWorking() }
                        }
                    }
                    .disabled(!snap.backendOk && !snap.running)
                    if snap.busy {
                        Label("busy", systemImage: "bolt.fill").foregroundStyle(.orange)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func activityList() -> some View {
        GroupBox("Activity") {
            VStack(alignment: .leading, spacing: 4) {
                if store.activity.isEmpty {
                    Text("Nothing yet.").foregroundStyle(.secondary)
                }
                ForEach(Array(store.activity.enumerated()), id: \.offset) { _, entry in
                    HStack(alignment: .top, spacing: 8) {
                        Text(entry.at.suffix(8)).font(.caption.monospaced()).foregroundStyle(.secondary)
                        Text(entry.text).font(.caption)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
