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

/// Good Idea Fairy: "a Kanban in the Swift Hive App... pick for them to be local hive or OH Hive."
/// Two halves: a real local idea board (add/triage/move, persisted by `KanbanStore`) and a
/// read-only glance at actual Hive cloud projects underneath, so both halves of "local projects
/// and cloud projects" are genuinely on screen together.
struct KanbanView: View {
    @EnvironmentObject private var store: HiveStore
    @StateObject private var kanban = KanbanStore()
    @State private var draftTitle = ""
    @State private var cloudProjects: [CloudProject] = []
    @State private var cloudError: String?
    @State private var loadingCloud = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                addRow
                localBoard
                Divider()
                cloudSection
            }
            .padding(20)
        }
        .navigationTitle("Kanban")
        .task { await loadCloud() }
    }

    // ---------- Local Hive: the idea board ----------

    private var addRow: some View {
        HStack {
            TextField("New idea\u{2026}", text: $draftTitle)
                .textFieldStyle(.roundedBorder)
                .onSubmit(addIdea)
            Button("Add", action: addIdea)
                .disabled(draftTitle.trimmingCharacters(in: .whitespaces).isEmpty)
        }
    }

    private func addIdea() {
        kanban.add(title: draftTitle)
        draftTitle = ""
    }

    private var localBoard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Local Hive").font(.headline)
            HStack(alignment: .top, spacing: 16) {
                ForEach(KanbanLane.allCases) { lane in
                    laneColumn(lane)
                }
            }
        }
    }

    private func laneColumn(_ lane: KanbanLane) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(lane.rawValue).font(.subheadline).foregroundStyle(.secondary)
            let cards = kanban.cards(in: lane)
            if cards.isEmpty {
                Text("\u{2014}").font(.caption).foregroundStyle(.tertiary)
            }
            ForEach(cards) { card in
                cardView(card)
            }
        }
        .frame(minWidth: 220, alignment: .top)
    }

    private func cardView(_ card: KanbanCard) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(card.title).font(.body.weight(.medium))
            if !card.notes.isEmpty {
                Text(card.notes).font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Picker("", selection: Binding(
                    get: { card.destination },
                    set: { kanban.setDestination(card, $0) }
                )) {
                    ForEach(KanbanDestination.allCases) { d in Text(d.rawValue).tag(d) }
                }
                .labelsHidden()
                .frame(width: 110)

                Picker("", selection: Binding(
                    get: { card.lane },
                    set: { kanban.setLane(card, $0) }
                )) {
                    ForEach(KanbanLane.allCases) { l in Text(l.rawValue).tag(l) }
                }
                .labelsHidden()
                .frame(width: 100)

                Spacer()
                Button(role: .destructive) { kanban.remove(card) } label: {
                    Image(systemName: "trash")
                }
                .buttonStyle(.plain)
            }
        }
        .padding(10)
        .background(Color.secondary.opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    // ---------- Hive: real cloud projects, read-only ----------

    private var cloudSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Hive").font(.headline)
                Spacer()
                if loadingCloud { ProgressView().controlSize(.small) }
                Button("Refresh") { Task { await loadCloud() } }
            }
            if let cloudError {
                Text(cloudError).font(.caption).foregroundStyle(.secondary)
            } else if cloudProjects.isEmpty && !loadingCloud {
                Text("No cloud projects visible yet \u{2014} pair this machine first, or check back once one exists.")
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
