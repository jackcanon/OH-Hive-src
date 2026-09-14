import Foundation

/// One saved chat's worth of history (2026-09-13, the Cowork-style sidebar redesign -- Jack:
/// "make sure we have Projects along the left side, and new chats etc"). `ChatEngine` previously
/// had no memory between launches at all (see its old doc comment); this is that follow-on,
/// same JSON-file-on-disk pattern `KanbanStore` already established for local-only state that has
/// no Rust/Supabase-side equivalent -- a saved chat session is purely a Swift-app convenience, not
/// Hive-distributed state.
struct PersistedChatMessage: Codable, Identifiable {
    enum Role: String, Codable { case user, assistant, system }
    let id: UUID
    let role: Role
    let text: String

    init(id: UUID = UUID(), role: Role, text: String) {
        self.id = id
        self.role = role
        self.text = text
    }
}

struct ChatSession: Codable, Identifiable, Equatable {
    let id: UUID
    var title: String
    var messages: [PersistedChatMessage]
    /// "on_device" | "byok" -- a stable key, not `ChatProvider`'s display-label raw value (that's
    /// UI copy and shouldn't double as a persistence format).
    var providerKey: String
    var byokProvider: String?
    var byokModel: String
    var createdAt: Date
    var updatedAt: Date

    static func == (lhs: ChatSession, rhs: ChatSession) -> Bool { lhs.id == rhs.id }

    init(id: UUID = UUID()) {
        self.id = id
        self.title = "New chat"
        self.messages = []
        // 2026-09-13: default flipped to "byok" (was "on_device") when on-device chat moved
        // behind an opt-in Settings toggle -- see ChatEngine.swift's `provider` doc comment.
        self.providerKey = "byok"
        self.byokProvider = nil
        self.byokModel = ""
        self.createdAt = Date()
        self.updatedAt = Date()
    }
}

@MainActor
final class ChatSessionStore: ObservableObject {
    /// Newest-updated first, same ordering Cowork/Codex/Hermes-style sidebars use for a chat list.
    @Published private(set) var sessions: [ChatSession] = []

    private let fileURL: URL

    init() {
        let dir = ChatSessionStore.appSupportDir()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        self.fileURL = dir.appendingPathComponent("chat_sessions.json")
        load()
    }

    private static func appSupportDir() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("OHHive", isDirectory: true)
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL) else { return }
        if let decoded = try? JSONDecoder().decode([ChatSession].self, from: data) {
            sessions = decoded.sorted { $0.updatedAt > $1.updatedAt }
        }
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(sessions) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }

    /// Every "+ New chat" click gets a real, immediately-listed session (even before a first
    /// message) -- simpler than tracking an "unsaved draft chat" state everywhere that reads the
    /// sidebar list.
    @discardableResult
    func createSession() -> ChatSession {
        let session = ChatSession()
        sessions.insert(session, at: 0)
        save()
        return session
    }

    func session(_ id: UUID) -> ChatSession? {
        sessions.first { $0.id == id }
    }

    /// Re-sorts to keep newest-updated first, and re-titles from the first user message once one
    /// exists -- so a chat started via "+ New chat" stops showing as "New chat" in the sidebar the
    /// moment it has real content, without the caller having to remember to do that itself.
    func update(_ session: ChatSession) {
        guard let idx = sessions.firstIndex(where: { $0.id == session.id }) else { return }
        var updated = session
        updated.updatedAt = Date()
        if updated.title == "New chat", let firstUser = updated.messages.first(where: { $0.role == .user }) {
            let trimmed = firstUser.text.trimmingCharacters(in: .whitespacesAndNewlines)
            updated.title = trimmed.count > 60 ? String(trimmed.prefix(60)) + "\u{2026}" : trimmed
        }
        sessions[idx] = updated
        sessions.sort { $0.updatedAt > $1.updatedAt }
        save()
    }

    func remove(_ session: ChatSession) {
        sessions.removeAll { $0.id == session.id }
        save()
    }
}
