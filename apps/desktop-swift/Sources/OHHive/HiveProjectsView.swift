import SwiftUI

/// One project as `hive.node_projects_overview` returns it -- decoded here rather than passed
/// through `JSONSerialization` (unlike `ServerView`'s `summaryJson`), since this shape is a new,
/// intentionally-designed RPC (not an open-ended existing document) and is stable enough to model.
private struct CloudProject: Decodable, Identifiable {
    let id: String
    let title: String
    let goal: String
    let executionMode: String
    let owner: String
    let fundBalance: Double
    let cards: [String: Int]?

    enum CodingKeys: String, CodingKey {
        case id, title, goal, owner, cards
        case executionMode = "execution_mode"
        case fundBalance = "fund_balance"
    }
}

/// The community Hive's own projects -- read-only here, matching web's project list. Split out of
/// the old combined `KanbanView` (2026-09-13, Jack: "Hive and Private Fleet should be two tabs that
/// differentiate everything... that way it really emphasizes the separation of Hive vs Private
/// Fleet. We are working towards a bring your own community concept, where Hive isn't specific to
/// just one group, but it can be re-used by other communities.") -- this view is deliberately
/// Hive-only now; a member who never pairs with the community Hive sees just the empty state below
/// and otherwise lives entirely in Private Fleet (see `PrivateFleetBoardView`/`PrivateFleetView`).
struct HiveProjectsView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var cloudProjects: [CloudProject] = []
    @State private var cloudError: String?
    @State private var loadingCloud = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Real projects funded and run on the community Hive.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                HStack {
                    Spacer()
                    if loadingCloud { ProgressView().controlSize(.small) }
                    Button("Refresh") { Task { await loadCloud() } }
                }
                if let cloudError {
                    Text(cloudError).font(.caption).foregroundStyle(.secondary)
                } else if cloudProjects.isEmpty && !loadingCloud {
                    Text("No cloud projects visible yet \u{2014} pair this machine with the Hive first, or check back once one exists. Don't want to join a community Hive at all? Everything under Private Fleet works standalone.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                ForEach(cloudProjects) { p in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(p.title).font(.body.weight(.medium))
                            modeBadge(p.executionMode)
                            Spacer()
                            Text(String(format: "%.0f \u{1F36F}", p.fundBalance)).font(.caption).foregroundStyle(.secondary)
                        }
                        Text(p.goal).font(.caption).foregroundStyle(.secondary)
                        HStack(spacing: 10) {
                            Text("by \(p.owner)").font(.caption2).foregroundStyle(.tertiary)
                            if let cards = p.cards {
                                Text(cards.map { "\($0.value) \($0.key)" }.sorted().joined(separator: " \u{00b7} "))
                                    .font(.caption2).foregroundStyle(.tertiary)
                            }
                        }
                    }
                    .padding(10)
                    .background(Color.secondary.opacity(0.05))
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                }
            }
            .padding(20)
        }
        .navigationTitle("Hive Projects")
        .task { await loadCloud() }
    }

    private func modeBadge(_ mode: String) -> some View {
        Text(mode == "local" ? "LOCAL" : "HIVE")
            .font(.system(size: 9, weight: .semibold))
            .padding(.horizontal, 6).padding(.vertical, 2)
            .background(Color.secondary.opacity(0.15))
            .clipShape(Capsule())
    }

    private func loadCloud() async {
        loadingCloud = true
        defer { loadingCloud = false }
        guard let json = await store.kanbanCloudProjects(), let data = json.data(using: .utf8) else {
            cloudError = "Couldn't load cloud projects \u{2014} pair this machine first."
            return
        }
        do {
            cloudProjects = try JSONDecoder().decode([CloudProject].self, from: data)
            cloudError = nil
        } catch {
            cloudError = "Couldn't read the project list (\(error.localizedDescription))."
        }
    }
}
