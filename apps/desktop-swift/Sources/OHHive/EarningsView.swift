import Foundation
import SwiftUI
import OHHiveFFI

/// Decodes the same `hive_node_summary` document the Tauri app's `Snapshot["summary"]` shows
/// (see `apps/desktop/src/App.tsx`'s `Snapshot.summary` type) -- `HiveSnapshot.summaryJson` is
/// that document passed through as a raw string (ADR-018: not worth modeling as a `uniffi::Record`
/// until this shape is stable enough to commit to across the FFI boundary), so it's decoded here
/// instead. Field names are kept snake_case to match the JSON exactly, no key-strategy guessing.
private struct NodeSummary: Decodable {
    struct NodeInfo: Decodable {
        let display_name: String
        let region: String?
        let presence: String
        let role: String
        /// This Mac's own round-trip time to the hub, as of its last heartbeat (nil until the
        /// first one lands). Jack, 2026-09-12: "how quickly can we get the telemetry wired into
        /// the swift app?" -- same number the web app's sidebar/Connectivity section show.
        let rtt_ms: Int?
    }
    struct Earned: Decodable {
        let total: Double
        let last_24h: Double
        let cards: Int
        let tokens_out: Int
    }
    struct RecentCard: Decodable {
        let at: String
        let card: String
        let project: String
        let honey: Double
        let tokens_out: Int
    }
    let node: NodeInfo?
    let earned: Earned
    let wallet: Double?
    let queue: Int
    let rate: Double?
    let recent: [RecentCard]
}

struct EarningsView: View {
    @EnvironmentObject private var store: HiveStore

    private var summary: NodeSummary? {
        guard let json = store.snapshot?.summaryJson, let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(NodeSummary.self, from: data)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let summary {
                    statGrid(summary)
                    recentCards(summary)
                    if let rate = summary.rate {
                        Text("Rate: \(rate, specifier: "%.4f") Honey per output token.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Link("Full wallet at ohghive.com/wallet", destination: URL(string: "https://ohghive.com/wallet")!)
                        .font(.caption)
                } else if store.snapshot?.paired == true {
                    ProgressView("Loading\u{2026}")
                } else {
                    Text("Pair this machine first.").foregroundStyle(.secondary)
                }
            }
            .padding(20)
        }
        .navigationTitle("Earnings")
    }

    @ViewBuilder
    private func statGrid(_ s: NodeSummary) -> some View {
        LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
            stat(String(format: "%.3f", s.earned.total), "earned by this Mac, all time")
            stat(String(format: "%.2f", s.wallet ?? 0), "your wallet")
            stat("\(s.earned.cards)", "cards finished")
            stat("\(s.earned.tokens_out)", "tokens generated")
            stat(s.node?.rtt_ms.map { "\($0)ms" } ?? "—", "hub round-trip time")
        }
    }

    private func stat(_ value: String, _ label: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(value).font(.title2.bold())
            Text(label).font(.caption).foregroundStyle(.secondary)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 8).fill(.quaternary.opacity(0.3)))
    }

    @ViewBuilder
    private func recentCards(_ s: NodeSummary) -> some View {
        GroupBox("Recent cards") {
            VStack(alignment: .leading, spacing: 4) {
                if s.recent.isEmpty {
                    Text("No cards finished yet.").foregroundStyle(.secondary)
                }
                ForEach(Array(s.recent.enumerated()), id: \.offset) { _, r in
                    HStack {
                        Text(r.card)
                        Text(r.project).font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Text("+\(r.honey, specifier: "%.4f")").foregroundStyle(.green)
                    }
                    .font(.caption)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
