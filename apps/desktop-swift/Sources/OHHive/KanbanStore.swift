import Foundation

/// Where an idea is headed once it stops being just an idea. Mirrors the Good Idea Fairy request
/// verbatim: "add ideas and I can pick for them to be local hive or OH Hive."
enum KanbanDestination: String, Codable, CaseIterable, Identifiable {
    case undecided = "Undecided"
    case localHive = "Local Hive"
    case ohHive = "OH Hive"
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

/// Purely local persistence -- these are ideas that haven't become real Hive state yet, so there's
/// nothing for the Rust core to own. Once a member decides to actually run one against OH Hive,
/// that becomes a real `hive.projects` row created the normal way (the "new project" flow the web
/// app already has); this store never talks to the hub. A future step (not built here) could add a
/// "send to OH Hive" action that calls that creation flow directly from this card's title/notes.
@MainActor
final class KanbanStore: ObservableObject {
    @Published private(set) var cards: [KanbanCard] = []

    private let fileURL: URL

    init() {
        let dir = KanbanStore.appSupportDir()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        self.fileURL = dir.appendingPathComponent("kanban.json")
        load()
    }

    private static func appSupportDir() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("OHHive", isDirectory: true)
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL) else { return }
        if let decoded = try? JSONDecoder().decode([KanbanCard].self, from: data) {
            cards = decoded.sorted { $0.createdAt < $1.createdAt }
        }
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(cards) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }

    func add(title: String, notes: String = "") {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        cards.append(KanbanCard(title: trimmed, notes: notes))
        save()
    }

    func update(_ card: KanbanCard) {
        guard let idx = cards.firstIndex(where: { $0.id == card.id }) else { return }
        cards[idx] = card
        save()
    }

    func remove(_ card: KanbanCard) {
        cards.removeAll { $0.id == card.id }
        save()
    }

    func setDestination(_ card: KanbanCard, _ destination: KanbanDestination) {
        var c = card; c.destination = destination; update(c)
    }

    func setLane(_ card: KanbanCard, _ lane: KanbanLane) {
        var c = card; c.lane = lane; update(c)
    }

    func cards(in lane: KanbanLane) -> [KanbanCard] {
        cards.filter { $0.lane == lane }
    }
}
