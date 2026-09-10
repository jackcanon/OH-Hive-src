import Foundation
import FoundationModels

/// First slice of the ADR-015 local chat/agent engine (ADR-018 decision 5/amendment decision 7):
/// on-device only for now, via Apple's real Foundation Models framework (`SystemLanguageModel` +
/// `LanguageModelSession`, shipping since macOS 26 and refined for macOS 27). This machine's own
/// node/server status is exposed to the model as a tool call -- nothing here ever touches
/// Hive-distributed cards or another member's paid work, per the ADR-018 guardrail: on-device and
/// Private Cloud Compute models serve `execution_mode='local'` runs (the owner's own machine)
/// only.
///
/// `DynamicProfile` routing across `SystemLanguageModel` / `PrivateCloudComputeLanguageModel` /
/// a Claude BYOK package (ADR-018 amendment decision 7) is NOT wired up yet -- those two
/// providers aren't real, shipped Apple/Anthropic APIs as of this build, just named in Loki's
/// macOS 27 research memo. `ChatProvider` below is the seam that routing will plug into once
/// they exist; today it only has one working case.
enum ChatProvider: String, CaseIterable, Identifiable {
    case systemOnDevice = "On-device (Apple Intelligence)"
    // case privateCloudCompute = "Private Cloud Compute"   // not yet available -- see above
    // case claude = "Claude (bring your own key)"          // not yet available -- see above
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

    private var session: LanguageModelSession?
    private let store: HiveStore

    init(store: HiveStore) {
        self.store = store
        checkAvailability()
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
                """
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
    }
}

struct ChatMessage: Identifiable {
    enum Role { case user, assistant, system }
    let id = UUID()
    let role: Role
    let text: String
}

/// Lets `ChatView` hold a `@StateObject` that starts empty and gets its real `ChatEngine`
/// assigned once the environment's `HiveStore` is available (see `ChatView.body`'s `.task`) --
/// `@StateObject`'s own initializer runs too early to reach into `@EnvironmentObject`.
@MainActor
final class ChatEngineHolder: ObservableObject {
    @Published var engine: ChatEngine?
}
