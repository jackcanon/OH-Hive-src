import Foundation

// Identical model/persistence shape to the Mac app's `apps/desktop-swift/Sources/OHHive/KanbanStore.swift`
// -- duplicated rather than shared across packages for now (no shared Swift package between the two
// apps exists yet) since it's pure Foundation with zero platform-specific code either way. Worth
// factoring into a shared package once both apps stabilize.

enum KanbanDestination: String, Codable, CaseIterable, Identifiable {
    case undecided = "Undecided"
    case localHive = "Local Hive"
    case ohHive = "Hive"
    var id: String { rawValue }
}

enum KanbanLane: String, Codable, CaseIterable, Identifiable {
    case inbox = "Inbox"
    case inProgress = "In Progress"
    case done = "Done"
    var id: String { rawValue }
}

struct KanbanCard: Identifiable, Codable, Equatable {
    let id: UUID
    var title: String
    var notes: String
    var destination: KanbanDestination
    var lane: KanbanLane
    var createdAt: Date

    init(title: String, notes: String = "", destination: KanbanDestination = .undecided, lane: KanbanLane = .inbox) {
        self.id = UUID()
        self.title = title
        self.notes = notes
        self.destination = destination
        self.lane = lane
        self.createdAt = Date()
    }
}

@MainActor
final class KanbanStore: ObservableObject {
    @Published private(set) var cards: [KanbanCard] = []
    private let fileURL: URL

    init() {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("OHHiveMobile", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        self.fileURL = dir.appendingPathComponent("kanban.json")
        load()
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let decoded = try? JSONDecoder().decode([KanbanCard].self, from: data) else { return }
        cards = decoded.sorted { $0.createdAt < $1.createdAt }
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(cards) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }

    func add(title: String) {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        cards.append(KanbanCard(title: trimmed)); save()
    }
    func update(_ card: KanbanCard) {
        guard let idx = cards.firstIndex(where: { $0.id == card.id }) else { return }
        cards[idx] = card; save()
    }
    func remove(_ card: KanbanCard) { cards.removeAll { $0.id == card.id }; save() }
    func setDestination(_ card: KanbanCard, _ d: KanbanDestination) { var c = card; c.destination = d; update(c) }
    func setLane(_ card: KanbanCard, _ l: KanbanLane) { var c = card; c.lane = l; update(c) }
    func cards(in lane: KanbanLane) -> [KanbanCard] { cards.filter { $0.lane == lane } }
}
