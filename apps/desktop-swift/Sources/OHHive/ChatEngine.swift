import Foundation
import FoundationModels
import OHHiveFFI

/// First slice of the ADR-015 local chat/agent engine (ADR-018 decision 5/amendment decision 7):
/// on-device only for now, via Apple's real Foundation Models framework (`SystemLanguageModel` +
/// `LanguageModelSession`, shipping since macOS 26 and refined for macOS 27). This machine's own
/// node/server status is exposed to the model as a tool call -- nothing here ever touches
/// Hive-distributed cards or another member's paid work, per the ADR-018 guardrail: on-device and
/// Private Cloud Compute models serve `execution_mode='local'` runs (the owner's own machine)
/// only.
///
/// `PrivateCloudComputeLanguageModel` (ADR-018 amendment decision 7) is still not a real, shipped
/// Apple API as of this build -- that case stays commented out below. The BYOK case, however, IS
/// wired up now (2026-09-13, Jack: "get the BYOK to swift"): rather than a dedicated Swift
/// `ClaudeLanguageModel` package, it routes through the shared Rust core's `HubClient` to the
/// `interview` Edge Function's node-key path -- same mechanism every other Hive network call
/// uses, and it covers Anthropic/OpenAI/Nous (whichever key the member has on file in Settings),
/// not just Claude.
enum ChatProvider: String, CaseIterable, Identifiable {
    case systemOnDevice = "On-device (Apple Intelligence)"
    case byok = "Your API key"
    // case privateCloudCompute = "Private Cloud Compute"   // not yet available -- see above
    var id: String { rawValue }
}

enum ChatEngineError: LocalizedError {
    case unavailable(String)
    var errorDescription: String? {
        switch self {
        case .unavailable(let why): return why
        }
    }
}

/// Lets the model answer questions about this machine's own Hive membership (paired?, working?,
/// models available, regional-server status) without the app hand-writing string templates for
/// every phrasing -- a small, concrete demonstration of Foundation Models tool calling wired to
/// real `HiveStore` state. Read-only: this tool cannot start/stop anything (that stays behind the
/// explicit buttons in NodeView/ServerView), it only reports.
struct NodeStatusTool: Tool {
    let name = "nodeStatus"
    let description = "Reports this Mac's current Hive status: whether it's paired, whether it's working, which models are available, and whether it's serving as a regional server."

    @Generable
    struct Arguments {}

    let store: HiveStore

    func call(arguments: Arguments) async throws -> String {
        let snap = await store.snapshot
        let sv = await store.server
        guard let snap else {
            return "Status isn't loaded yet -- ask again in a moment."
        }
        var lines: [String] = []
        lines.append(snap.paired ? "Paired with the Hive." : "Not paired yet.")
        if snap.paired {
            lines.append(snap.running ? "Currently working (leasing cards)." : "Paired but idle (not working right now).")
            lines.append("\(snap.models.count) model(s) available: \(snap.models.joined(separator: ", "))")
            if let region = snap.region { lines.append("Region: \(region).") }
        }
        if let sv, sv.running {
            lines.append("Also serving as a regional server (\(sv.tier)), holding \(sv.blobs) blob(s).")
        }
        return lines.joined(separator: " ")
    }
}

/// One chat session's worth of state. Not persisted across launches yet (ADR-018 amendment
/// decision 7 calls for on-device history indexed in Core Spotlight -- that's a follow-on, this
/// is the first working slice).
@MainActor
final class ChatEngine: ObservableObject {
    @Published var messages: [ChatMessage] = []
    @Published var isResponding = false
    @Published var availabilityNote: String?
    // Default is BYOK, not on-device -- 2026-09-13, Jack: "hide it for now behind a setting for
    // end users to turn on if they want to." On-device stays fully implemented (see this file's
    // header doc) but is opt-in now via a Settings toggle (`ChatView`'s `showOnDeviceOption`
    // `@AppStorage`, same key as `SettingsView`'s toggle) rather than the default first choice.
    @Published var provider: ChatProvider = .byok

    // Two-stage provider/model picker state (2026-09-13, see ProviderModelPicker.swift).
    // `byokKeysStatus` is `nil` until the first BYOK send/tab-appear loads it; `byokProvider`
    // defaults to the first configured provider once that load completes, so a member with
    // exactly one key on file never has to pick anything. `byokModel` empty means "use that
    // provider's saved preferred_model, or the interview function's own default."
    @Published var byokKeysStatus: ByokKeysStatus?
    @Published var byokProvider: String?
    @Published var byokModel: String = ""

    // Multi-session persistence (2026-09-13, the Cowork-style sidebar redesign -- see
    // ChatSessionStore.swift). `currentSessionId` is always a real, already-created
    // `ChatSession.id` once `open(_:)` has run at least once -- `ChatView` guarantees this by
    // always resolving "+ New chat" to a freshly-created session before showing a chat at all.
    @Published var currentSessionId: UUID?
    @Published var sessionTitle: String = "New chat"

    private var session: LanguageModelSession?
    private let store: HiveStore
    private let sessions: ChatSessionStore
    // Persistent chat memory (2026-09-13, Hermes-agent survey) -- fetched once per app launch,
    // lazily, right before the first message is sent (not eagerly in init, since HiveStore's
    // node-key call needs the node to already be paired and this shouldn't block opening the tab).
    // `nil` until loaded; `memoryLoaded` distinguishes "not fetched yet" from "fetched, empty."
    private var memory: ChatMemory?
    private var memoryLoaded = false
    private var byokKeysLoaded = false

    init(store: HiveStore, sessions: ChatSessionStore) {
        self.store = store
        self.sessions = sessions
        checkAvailability()
    }

    /// Switches to a different saved session (or the same one -- a cheap no-op). `ChatView` calls
    /// this from a `.task(id: sessionId)` whenever the sidebar selection changes, so opening a
    /// chat is just "load its saved state into this engine's published properties."
    func open(_ id: UUID) {
        guard id != currentSessionId, let found = sessions.session(id) else {
            if id == currentSessionId { return }
            // Session vanished (e.g. removed elsewhere) -- fall back to a blank, unsaved-looking
            // state rather than crashing or showing stale messages from the previous chat.
            currentSessionId = id
            sessionTitle = "New chat"
            messages = []
            return
        }
        currentSessionId = id
        sessionTitle = found.title
        messages = found.messages.map { ChatMessage(role: ChatMessage.Role($0.role), text: $0.text) }
        provider = found.providerKey == "byok" ? .byok : .systemOnDevice
        byokProvider = found.byokProvider
        byokModel = found.byokModel
    }

    /// Writes the engine's current in-memory state back to the session store. Called after every
    /// turn (both on-device and BYOK) so a chat survives switching away and back, or quitting the
    /// app -- see `ChatSessionStore`'s doc for why this exists at all.
    private func persistCurrentSession() {
        guard let id = currentSessionId else { return }
        var record = sessions.session(id) ?? ChatSession(id: id)
        record.messages = messages.map { PersistedChatMessage(role: PersistedChatMessage.Role($0.role), text: $0.text) }
        record.providerKey = provider == .byok ? "byok" : "on_device"
        record.byokProvider = byokProvider
        record.byokModel = byokModel
        sessions.update(record)
        sessionTitle = sessions.session(id)?.title ?? record.title
    }

    /// Lazily loads BYOK key status (same lazy-on-first-need pattern as `loadMemoryIfNeeded`) and
    /// defaults `byokProvider` to the first configured provider if nothing's been picked yet.
    /// Safe to call every time the picker or the BYOK send path needs current data -- cheap no-op
    /// once loaded, and `ChatView` can also call this eagerly when the provider segment switches
    /// to BYOK so the picker has data before the member's first send.
    func loadByokKeysIfNeeded() async {
        guard !byokKeysLoaded else { return }
        byokKeysLoaded = true
        byokKeysStatus = await store.byokKeysStatus()
        if byokProvider == nil {
            let status = byokKeysStatus
            if status?.anthropic != nil { byokProvider = "anthropic" }
            else if status?.openai != nil { byokProvider = "openai" }
            else if status?.nous != nil { byokProvider = "nous" }
        }
    }

    private func loadMemoryIfNeeded() async {
        guard !memoryLoaded else { return }
        memoryLoaded = true
        memory = await store.chatMemory()
    }

    // Same framing as the `interview` Edge Function's `memoryAppendix()` -- background context
    // the model should use naturally, not recite back. Keeps on-device chat's continuity in sync
    // with whatever BYOK sessions have taught the assistant about this member.
    private func memoryAppendix() -> String {
        guard let memory, !(memory.memoryMd.isEmpty && memory.userMd.isEmpty) else { return "" }
        var parts: [String] = []
        if !memory.userMd.isEmpty { parts.append("About them: \(memory.userMd)") }
        if !memory.memoryMd.isEmpty { parts.append("Notes from past sessions: \(memory.memoryMd)") }
        return "\n\nWhat you already know about this member from earlier sessions (use naturally where relevant; don't recite it back or announce that you \"remember\" things):\n\n" + parts.joined(separator: "\n\n")
    }

    private func checkAvailability() {
        switch SystemLanguageModel.default.availability {
        case .available:
            availabilityNote = nil
            session = LanguageModelSession(
                tools: [NodeStatusTool(store: store)],
                instructions: """
                You are the assistant built into Hive, a native Mac app for a member of a \
                volunteer compute-sharing network. Answer questions about this machine's own \
                Hive membership using the nodeStatus tool when relevant. Keep answers short and \
                plain -- this is a small utility panel, not a chat product. Never claim you can \
                start or stop the node or the server role yourself; direct the person to the \
                Node or Server section of the app for that.
                """ + memoryAppendix()
            )
        case .unavailable(let reason):
            session = nil
            availabilityNote = Self.describe(reason)
        @unknown default:
            session = nil
            availabilityNote = "Apple Intelligence isn't available on this machine right now."
        }
    }

    private static func describe(_ reason: SystemLanguageModel.Availability.UnavailableReason) -> String {
        switch reason {
        case .deviceNotEligible:
            return "This Mac isn't eligible for Apple Intelligence."
        case .appleIntelligenceNotEnabled:
            return "Turn on Apple Intelligence in System Settings to use the assistant."
        case .modelNotReady:
            return "The on-device model is still downloading -- try again shortly."
        @unknown default:
            return "The on-device model isn't available right now."
        }
    }

    func send(_ text: String) async {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        switch provider {
        case .systemOnDevice: await sendOnDevice(trimmed)
        case .byok: await sendByok(trimmed)
        }
    }

    private func sendOnDevice(_ trimmed: String) async {
        await loadMemoryIfNeeded()
        checkAvailability()
        guard let session else {
            messages.append(ChatMessage(role: .system, text: availabilityNote ?? "Assistant unavailable."))
            return
        }
        messages.append(ChatMessage(role: .user, text: trimmed))
        isResponding = true
        defer { isResponding = false }
        do {
            let response = try await session.respond(to: trimmed)
            messages.append(ChatMessage(role: .assistant, text: response.content))
        } catch {
            messages.append(ChatMessage(role: .system, text: "Couldn't get a response: \(error.localizedDescription)"))
        }
        persistCurrentSession()
    }

    /// Stateless -- resends the whole visible transcript (skipping system-role notices) plus the
    /// new turn every time, same shape as the web app's /new page (`crates/ohhive-ffi/src/chat.rs`
    /// has the full reasoning). No tool-calling, no streaming: one request, one reply.
    private func sendByok(_ trimmed: String) async {
        await loadByokKeysIfNeeded()
        messages.append(ChatMessage(role: .user, text: trimmed))
        isResponding = true
        defer { isResponding = false }
        let history: [ByokChatTurn] = messages.compactMap { m in
            switch m.role {
            case .user: return ByokChatTurn(role: "user", content: m.text)
            case .assistant: return ByokChatTurn(role: "assistant", content: m.text)
            case .system: return nil
            }
        }
        do {
            let result: ChatReply
            if let byokProvider {
                // Explicit provider from the two-stage picker -- never silently falls back to a
                // different configured key, matching the Edge Function's own narrowing behavior.
                result = try await store.sendByokChat(history: history, provider: byokProvider, model: byokModel)
            } else {
                // No provider chosen yet (e.g. `byokKeysStatus` came back with nothing configured)
                // -- same auto/priority-order path this always used before the picker existed.
                result = try await store.sendByokChat(history: history)
            }
            messages.append(ChatMessage(role: .assistant, text: result.reply))
        } catch {
            messages.append(ChatMessage(role: .system, text: "Couldn't get a response: \(error.localizedDescription)"))
        }
        persistCurrentSession()
    }
}

struct ChatMessage: Identifiable {
    enum Role { case user, assistant, system }
    let id = UUID()
    let role: Role
    let text: String
}

private extension ChatMessage.Role {
    init(_ persisted: PersistedChatMessage.Role) {
        switch persisted {
        case .user: self = .user
        case .assistant: self = .assistant
        case .system: self = .system
        }
    }
}

private extension PersistedChatMessage.Role {
    init(_ chat: ChatMessage.Role) {
        switch chat {
        case .user: self = .user
        case .assistant: self = .assistant
        case .system: self = .system
        }
    }
}

/// Lets `ChatView` hold a `@StateObject` that starts empty and gets its real `ChatEngine`
/// assigned once the environment's `HiveStore` is available (see `ChatView.body`'s `.task`) --
/// `@StateObject`'s own initializer runs too early to reach into `@EnvironmentObject`.
@MainActor
final class ChatEngineHolder: ObservableObject {
    @Published var engine: ChatEngine?
}
