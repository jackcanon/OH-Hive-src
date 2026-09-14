import SwiftUI

/// Good Idea Fairy's local idea board -- now scoped explicitly to Private Fleet, split out of the
/// old combined `KanbanView` (2026-09-13, Jack: "the Kanban for Private Fleet should be in a
/// separate section than the Hive Kanban... it really emphasizes the separation of Hive vs Private
/// Fleet"). This board was always local-only, single-machine, no Hive/Supabase involvement -- that
/// made it Private Fleet's board all along, just mislabeled "Local Hive" before. Pure `KanbanStore`
/// (JSON-on-disk) persistence, no pairing or network needed, so it works standalone for someone who
/// never connects to a community Hive at all (see `HiveProjectsView` for that half).
///
/// File kept as `KanbanView.swift` (the type backing it, `KanbanStore`/`KanbanCard`/`KanbanLane`/
/// `KanbanDestination`, lives in `KanbanStore.swift` and the name stuck) -- a rename is cosmetic
/// and not worth the extra diff right now.
struct PrivateFleetBoardView: View {
    @StateObject private var kanban = KanbanStore()
    @State private var draftTitle = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Your own local idea board -- this machine only, nothing synced to the community Hive.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                addRow
                board
            }
            .padding(20)
        }
        .navigationTitle("Private Fleet Projects")
    }

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

    private var board: some View {
        HStack(alignment: .top, spacing: 16) {
            ForEach(KanbanLane.allCases) { lane in
                laneColumn(lane)
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
}
