import Foundation
import Observation
import OHHiveFFI

/// App-owned connection and worker; selection polling belongs to the visible screen.
@MainActor @Observable
final class BotsModel {
    private let openSession: () async throws -> BotsSession
    private var session: BotsSession?
    private var opening: Task<BotsSession, Error>?
    private var worker: Task<Void, Never>?
    private var generation = UUID()
    private var selectionGeneration = UUID()
    private(set) var paired = false
    private(set) var agents: [BotsAgent] = []
    private(set) var hostID: String?
    private(set) var ownerID: String?
    private(set) var primaryEndpoint: String?
    var storageLabel: String { primaryEndpoint == nil ? "Private conversations stored on this Mac" : "Private conversations stored on your selected primary" }
    private(set) var conversation: BotsConversation?
    private(set) var messages: [BotsMessage] = []
    private(set) var loading = false
    private(set) var registering = false
    private(set) var sending = false
    var error: String?
    var sendError: String?
    var workerStatus = "Connect this Mac to open Bots."
    var selectedID: String?
    var draft = ""
    private var drafts: [String: String] = [:]
    private var retry: [String: BotsSend] = [:]

    init(node: HiveNode) { openSession = { try await node.botsOpen() } }
    init(openSession: @escaping () async throws -> BotsSession) { self.openSession = openSession }
    isolated deinit { worker?.cancel(); opening?.cancel() }

    func setPrimary(_ endpoint: String?) {
        guard primaryEndpoint != endpoint else { return }
        let wasPaired = paired
        setPaired(false)
        primaryEndpoint = endpoint
        setPaired(wasPaired)
    }

    func setPaired(_ value: Bool) {
        guard paired != value else { return }
        paired = value
        if value { startWorker() }
        else {
            generation = UUID(); selectionGeneration = UUID()
            worker?.cancel(); worker = nil
            opening?.cancel(); opening = nil; session = nil
            agents = []; messages = []; conversation = nil; selectedID = nil
            drafts = [:]; retry = [:]; draft = ""; hostID = nil; ownerID = nil
            loading = false; error = nil; sendError = nil; workerStatus = "Connect this Mac to open Bots."
        }
    }

    private func connection() async throws -> BotsSession {
        guard paired else { throw BotsUIError("Connect this Mac in Settings to open Bots.") }
        if let session { return session }
        let token = generation
        let task: Task<BotsSession, Error>
        if let opening { task = opening }
        else {
            task = Task { try await openSession() }
            opening = task
        }
        do {
            let result = try await task.value
            guard token == generation else { throw CancellationError() }
            session = result; opening = nil; hostID = result.hostId(); ownerID = result.ownerId()
            return result
        } catch {
            if token == generation { opening = nil }
            throw error
        }
    }

    private func startWorker() {
        guard worker == nil else { return }
        let token = generation
        worker = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, self.paired, self.generation == token else { return }
                do {
                    let session = try await self.connection()
                    if session.usesRemotePrimary() {
                        _ = try await session.agentsList()
                        guard self.generation == token, !Task.isCancelled else { return }
                        self.workerStatus = "Shared history connected. Remote agent execution is not enabled yet."
                        do { try await Task.sleep(for: .seconds(5)) } catch { return }
                        continue
                    }
                    self.workerStatus = "Local replies enabled"
                    let result = try await session.drainOnce()
                    guard self.generation == token, !Task.isCancelled else { return }
                    if result.failed > 0 { self.workerStatus = "A reply failed. Check your local model before sending again." }
                    else if result.requeued > 0 { self.workerStatus = "Waiting for this Mac’s available capacity…" }
                    else { self.workerStatus = "Local replies enabled" }
                } catch {
                    guard self.generation == token, !Task.isCancelled else { return }
                    self.workerStatus = botsErrorText(error)
                }
                do { try await Task.sleep(for: .seconds(5)) } catch { return }
            }
        }
    }

    func reconnect(preserveDrafts: Bool = true) async {
        if preserveDrafts { saveDraft() }
        let previousSelection = selectedID
        generation = UUID(); selectionGeneration = UUID()
        worker?.cancel(); worker = nil; opening?.cancel(); opening = nil; session = nil
        messages = []; conversation = nil; agents = []; hostID = nil; ownerID = nil; selectedID = nil
        if !preserveDrafts { drafts = [:]; retry = [:]; draft = "" }
        error = nil; sendError = nil; loading = false
        if paired {
            startWorker(); await refreshAgents()
            if preserveDrafts, let previousSelection, agents.contains(where: { $0.id == previousSelection }) {
                selectedID = previousSelection; draft = drafts[previousSelection] ?? ""
            }
        }
    }

    func refreshAgents() async {
        let token = generation
        do {
            let s = try await connection()
            let list = try await s.agentsList()
            guard token == generation, !Task.isCancelled else { return }
            agents = list.filter { !$0.archived }
            if selectedID == nil { selectedID = agents.first?.id }
            error = nil
        } catch {
            if token == generation, !Task.isCancelled { self.error = botsErrorText(error) }
        }
    }

    func update(agentID: String, name: String?, capabilityPolicyRef: String?) async throws {
        let token = generation
        let s = try await connection()
        let updated = try await s.agentsUpdate(agentId: agentID, name: name, capabilityPolicyRef: capabilityPolicyRef)
        guard token == generation, paired else { throw CancellationError() }
        // Do not reload the DM or replace a user's in-progress message when metadata changes.
        if let index = agents.firstIndex(where: { $0.id == updated.id }) { agents[index] = updated }
    }

    func register() async {
        guard !registering else { return }
        registering = true; defer { registering = false }
        let token = generation
        do {
            let s = try await connection()
            let agent = try await s.agentsCreate(name: Host.current().localizedName ?? "This Mac")
            guard token == generation else { return }
            await refreshAgents(); selectedID = agent.id
        } catch { if token == generation { self.error = botsErrorText(error) } }
    }

    /// Called by .task(id:); canceling selection cannot publish old results into a new DM.
    func watch(agentID: String?) async {
        let token = UUID(); selectionGeneration = token
        conversation = nil; messages = []; error = nil; sendError = nil
        draft = agentID.flatMap { drafts[$0] } ?? ""
        guard let agentID else { return }
        loading = true
        defer { if selectionGeneration == token { loading = false } }
        do {
            let s = try await connection()
            let all = try await s.conversationsList()
            try Task.checkCancellation()
            let c: BotsConversation
            if let existing = all.first(where: { $0.kind == "agent_dm" && $0.storageScope == "local_only" && $0.coordinator == agentID }) { c = existing }
            else { c = try await s.conversationsCreate(agentId: agentID) }
            guard selectionGeneration == token, !Task.isCancelled else { return }
            conversation = c
            // Core returns ascending order; consume all pages so initial history has no gaps.
            var cursor: UInt64? = nil
            repeat {
                let batch = try await s.messagesList(conversationId: c.id, page: BotsPage(before: nil, after: cursor, limit: 200))
                guard selectionGeneration == token, !Task.isCancelled else { return }
                merge(batch)
                cursor = messages.last?.serverSequence
                if batch.count < 200 { break }
            } while !Task.isCancelled
            loading = false
            while !Task.isCancelled {
                do {
                    let batch = try await s.messagesList(conversationId: c.id, page: BotsPage(before: nil, after: messages.last?.serverSequence, limit: 200))
                    guard selectionGeneration == token, !Task.isCancelled else { return }
                    merge(batch); error = nil
                } catch {
                    guard selectionGeneration == token, !Task.isCancelled else { return }
                    self.error = botsErrorText(error)
                }
                try await Task.sleep(for: .seconds(2))
            }
        } catch is CancellationError { }
        catch { if selectionGeneration == token, !Task.isCancelled { self.error = botsErrorText(error) } }
    }

    private func merge(_ batch: [BotsMessage]) {
        guard !batch.isEmpty else { return }
        var ids = Set(messages.map(\.id))
        messages.append(contentsOf: batch.filter { ids.insert($0.id).inserted })
        messages.sort { $0.serverSequence < $1.serverSequence }
    }

    func saveDraft() { if let selectedID { drafts[selectedID] = draft } }

    func send() async {
        guard !sending, let c = conversation, let agentID = selectedID else { return }
        let body = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty, body.utf8.count <= 65536 else { return }
        let token = selectionGeneration
        let connectionToken = generation
        sending = true; defer { sending = false }
        let d: BotsSend
        if let pending = retry[agentID], pending.body == body, pending.conversationId == c.id { d = pending }
        else {
            d = BotsSend(conversationId: c.id, clientRequestId: UUID().uuidString, expectedPolicyRevision: c.policyRevision, recipientIds: [agentID], body: body, threadRoot: nil)
            retry[agentID] = d
        }
        do {
            let s = try await connection()
            _ = try await s.messageSend(draft: d)
            guard connectionToken == generation else { return }
            retry[agentID] = nil
            if drafts[agentID]?.trimmingCharacters(in: .whitespacesAndNewlines) == body { drafts[agentID] = "" }
            guard selectionGeneration == token else { return }
            if draft.trimmingCharacters(in: .whitespacesAndNewlines) == body { draft = "" }
            sendError = nil
            // Poll from the previous sequence; inserting only the send receipt can skip replies.
        } catch {
            if selectionGeneration == token { self.sendError = "Message not confirmed. Retry the same text safely. \(botsErrorText(error))" }
        }
    }
}

private struct BotsUIError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

private func botsErrorText(_ error: Error) -> String {
    if let hive = error as? HiveError, case .Failed(let message) = hive { return message }
    return error.localizedDescription
}
