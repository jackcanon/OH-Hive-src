import SwiftUI

struct KanbanView: View {
    @StateObject private var kanban = KanbanStore()
    @State private var draftTitle = ""
    @State private var cloudProjects: [CloudProject] = []
    @State private var cloudError: String?
    @State private var loadingCloud = false

    var body: some View {
        List {
            Section("Add idea") {
                HStack {
                    TextField("New idea\u{2026}", text: $draftTitle)
                    Button("Add") { kanban.add(title: draftTitle); draftTitle = "" }
                        .disabled(draftTitle.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }

            ForEach(KanbanLane.allCases) { lane in
                let cards = kanban.cards(in: lane)
                if !cards.isEmpty {
                    Section(lane.rawValue) {
                        ForEach(cards) { card in
                            cardRow(card)
                        }
                    }
                }
            }

            Section("Hive (live)") {
                if loadingCloud { ProgressView() }
                if let cloudError { Text(cloudError).font(.caption).foregroundStyle(.secondary) }
                ForEach(cloudProjects) { p in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(p.title).font(.body.weight(.medium))
                        Text(p.goal).font(.caption).foregroundStyle(.secondary)
                        HStack {
                            if let role = p.myRole { Text("you: \(role)").font(.caption2) }
                            Spacer()
                            Text(String(format: "%.0f \u{1F36F} in fund", p.fundBalance)).font(.caption2)
                        }
                    }
                }
            }
        }
        .navigationTitle("Kanban")
        .task { await loadCloud() }
        .refreshable { await loadCloud() }
    }

    private func cardRow(_ card: KanbanCard) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(card.title)
            HStack {
                Picker("Destination", selection: Binding(get: { card.destination }, set: { kanban.setDestination(card, $0) })) {
                    ForEach(KanbanDestination.allCases) { d in Text(d.rawValue).tag(d) }
                }
                .pickerStyle(.menu)
                Spacer()
                Picker("Lane", selection: Binding(get: { card.lane }, set: { kanban.setLane(card, $0) })) {
                    ForEach(KanbanLane.allCases) { l in Text(l.rawValue).tag(l) }
                }
                .pickerStyle(.menu)
            }
            .font(.caption)
        }
        .swipeActions {
            Button(role: .destructive) { kanban.remove(card) } label: { Label("Delete", systemImage: "trash") }
        }
    }

    private func loadCloud() async {
        loadingCloud = true
        defer { loadingCloud = false }
        do {
            cloudProjects = try await supabase.rpc("hive_projects_overview").execute().value
            cloudError = nil
        } catch {
            cloudError = "Couldn't load cloud projects (\(error.localizedDescription))."
        }
    }
}
