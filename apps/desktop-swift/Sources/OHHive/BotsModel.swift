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
    private(set) var biographies: [String: AgentBiography] = [:]
    private var biographyRevisions: [String: UInt32] = [:]
    private(set) var agents: [BotsAgent] = []
    private(set) var rooms: [BotsConversation] = []
    private(set) var roomAgents: [BotsAgent] = []
    var mentionNote: String?
    var isRoom: Bool { selectedID?.hasPrefix("room:") == true }
    var roomTitle: String? { rooms.first { "room:" + $0.id == selectedID }?.title }
    private(set) var hostID: String?
    private(set) var ownerID: String?
    private(set) var primaryEndpoint: String?
    var storageLabel: String { primaryEndpoint == nil ? "Private conversations stored on this Mac" : "Private conversations stored on your selected primary" }
    private(set) var conversation: BotsConversation?
    private(set) var messages: [BotsMessage] = []
    private(set) var deliveryNotes: [String: String] = [:]
    static func deliveryNote(_ rows: [[String]]) -> [String: String] {
        var notes: [String: [String]] = [:]
        for row in rows where row.count == 3 {
            let state: String
            switch row[2] {
            case "done": state = "Replied"
            case "failed": state = "Reply failed — check the agent’s model or provider settings, then send a new message"
            case "running", "leased", "claimed": state = "Reply in progress"
            case "cancelled": state = "Cancelled"
            case "held": state = "Paused — the conversation’s turn limit was reached"
            case "unknown": state = "Reply outcome unknown — review before sending again"
            case "pending": state = "Waiting for agent"
            default: state = "Reply status unavailable"
            }
            notes[row[0], default: []].append("\(row[1]): \(state)")
        }
        return notes.mapValues { $0.joined(separator: "\n") }
    }
    private(set) var loading = false
    private(set) var registering = false
    private(set) var sending = false
    var error: String?
    /// Set when provider-agent provisioning failed but the roster still loaded. Distinct from
    /// `error`, which means the roster itself could not be read.
    var provisioningNote: String?
    var sendError: String?
    var workerStatus = "Connect this Mac to open Bots."
    private var workerFailure: String?
    private var workerPasses = 0
    var selectedID: String?
    var draft = ""
    private var drafts: [String: String] = [:]
    private var retry: [String: BotsSend] = [:]
    private var roomRetry: (details: [String], requestID: String)?
    private var creatingRoom = false

    init(node: HiveNode) { openSession = { try await node.botsOpen() } }
    init(openSession: @escaping () async throws -> BotsSession) { self.openSession = openSession }
    isolated deinit { worker?.cancel(); opening?.cancel() }

    static func canMessageAgent(_ agent: BotsAgent?) -> Bool {
        guard let agent, !agent.archived else { return false }
        return agent.runtimeKind == "local" && agent.preferredHost != nil
    }

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
            biographies = [:]; biographyRevisions = [:]; agents = []; rooms = []; roomAgents = []; mentionNote = nil; messages = []; conversation = nil; selectedID = nil
            workerFailure = nil
            drafts = [:]; retry = [:]; roomRetry = nil; draft = ""; hostID = nil; ownerID = nil
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
            task = Task {
                let result = try await openSession()
                // A paired secondary needs an agent in the primary's roster bound to
                // its authenticated host ID. Never reuse an old agent by display name.
                if result.usesRemotePrimary() {
                    let agents = try await result.agentsList()
                    let wasDeleted = try await result.hostAgentDeleted()
                    if !wasDeleted && !agents.contains(where: { $0.runtimeKind == "local" && $0.preferredHost == result.hostId() }) {
                        _ = try await result.agentsCreate(name: Host.current().localizedName ?? "This Mac")
                    }
                }
                return result
            }
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
                    // A remote primary used to stop here: the app would show a peer's shared
                    // history and never answer in it, because nothing could execute against a
                    // vault on another machine. That is no longer true -- the delivery surface
                    // is on the transport and the CLI has been draining a remote hub in the
                    // field -- so the drain below runs either way. This node still answers only
                    // for agents the hub says it hosts; that is decided there, from the
                    // session's key, not here.
                    let result = try await session.drainOnce()
                    guard self.generation == token, !Task.isCancelled else { return }
                    if result.failed > 0 { self.workerFailure = "A reply failed. Check Settings → Models on the agent’s computer, then send a new message." }
                    else if result.delivered > 0 { self.workerFailure = nil }
                    self.workerStatus = self.workerFailure ?? (result.requeued > 0
                        ? "Waiting for the model or provider. Your message is queued."
                        : "Connected. Replies run on each agent’s computer.")
                    self.workerPasses += 1
                    if self.agents.isEmpty || self.workerPasses % 3 == 0 { await self.refreshAgents() }
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
        biographies = [:]; biographyRevisions = [:]; messages = []; conversation = nil; roomAgents = []; mentionNote = nil; agents = []; rooms = []; hostID = nil; ownerID = nil; selectedID = nil
        if !preserveDrafts { drafts = [:]; retry = [:]; roomRetry = nil; draft = "" }
        error = nil; sendError = nil; loading = false
        if paired {
            startWorker(); await refreshAgents()
            if preserveDrafts, let previousSelection, (agents.contains(where: { $0.id == previousSelection }) || rooms.contains(where: { "room:" + $0.id == previousSelection })) {
                selectedID = previousSelection; draft = drafts[previousSelection] ?? ""
            }
        }
    }

    func refreshAgents() async {
        let token = generation
        do {
            let s = try await connection()
            // Provision an agent for every BYOK provider key on file *before* listing, which is
            // the order `ensure_provider_agents` documents: provisioning after the list would
            // hide a freshly-created agent until a second refresh. The Tauri shell has always
            // done this; the Den did not, which is why configuring an Anthropic or Nous key in
            // Settings never produced an agent you could actually see here.
            //
            // Deliberately tolerant: this is an enrichment step, not a precondition. If it fails
            // -- no keys configured, storage busy, a provider we cannot parse -- the local agents
            // the member already has must still be listed rather than the whole screen erroring.
            do {
                _ = try await s.ensureProviderAgents()
            } catch {
                provisioningNote = botsErrorText(error)
            }
            let list = try await s.agentsList()
            let conversations = try await s.conversationsList()
            guard token == generation, !Task.isCancelled else { return }
            agents = list.filter { !$0.archived }
            for agent in agents where biographyRevisions[agent.id] != agent.roleRevision {
                if let json = try? await s.agentBioGet(agentId: agent.id),
                   let bio = try? JSONDecoder().decode(AgentBiography.self, from: Data(json.utf8)) {
                    guard token == generation else { return }
                    biographies[agent.id] = bio; biographyRevisions[agent.id] = agent.roleRevision
                }
            }
            rooms = conversations.filter { $0.kind != "agent_dm" && $0.storageScope == "local_only" }
            if selectedID == nil { selectedID = agents.first?.id ?? rooms.first.map { "room:" + $0.id } }
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

    func agentBio(_ id: String) async throws -> AgentBiography {
        let json = try await connection().agentBioGet(agentId: id)
        return try JSONDecoder().decode(AgentBiography.self, from: Data(json.utf8))
    }
    func saveAgentBio(_ id: String, name: String, profile: AgentBiography) async throws {
        let token = generation
        let json = String(decoding: try JSONEncoder().encode(profile), as: UTF8.self)
        try await connection().agentBioSet(agentId: id, name: name, profile: json)
        guard token == generation else { throw CancellationError() }
        await refreshAgents()
    }
    func deleteAgent(_ id: String) async throws {
        let token = generation
        try await connection().agentsArchive(agentId: id)
        guard token == generation else { throw CancellationError() }
        agents.removeAll { $0.id == id }
        if selectedID == id { selectedID = nil; conversation = nil; messages = [] }
        await refreshAgents()
    }
    func userProfile() async throws -> String { try await connection().userProfileGet() }
    func saveUserProfile(name: String, about: String) async throws {
        try await connection().userProfileSet(preferredName: name, about: about)
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

    func createRoom(title: String, agentIDs: [String], projectID: String?, coordinatorID: String?) async throws {
        guard !creatingRoom else { throw BotsUIError("Room creation is already in progress.") }
        creatingRoom = true; defer { creatingRoom = false }
        let token = generation
        let s = try await connection()
        let details = [s.ownerId(), primaryEndpoint ?? "local", title.trimmingCharacters(in: .whitespacesAndNewlines), projectID ?? "", coordinatorID ?? ""] + Array(Set(agentIDs)).sorted()
        if roomRetry?.details != details { roomRetry = (details, UUID().uuidString) }
        let requestID = roomRetry!.requestID
        let room = try await s.roomsCreate(requestId: requestID, title: title, kind: projectID == nil ? "team" : "project", agentIds: agentIDs, projectId: projectID, coordinatorId: coordinatorID)
        guard generation == token else { throw CancellationError() }
        saveDraft()
        roomRetry = nil
        rooms.removeAll { $0.id == room.id }
        rooms.insert(room, at: 0); selectedID = "room:" + room.id
    }

    func authorName(_ message: BotsMessage) -> String {
        if message.kind == "system" { return "System" }
        if message.authorKind == "user" { return message.authorId == ownerID ? "You" : "Member" }
        return (roomAgents + agents).first { $0.id == message.authorId }?.name ?? "Agent \(message.authorId.prefix(8))"
    }

    /// Called by .task(id:); canceling selection cannot publish old results into a new DM.
    func watch(agentID: String?) async {
        let token = UUID(); selectionGeneration = token
        deliveryNotes = [:]
        conversation = nil; roomAgents = []; mentionNote = nil; messages = []; error = nil; sendError = nil
        draft = agentID.flatMap { drafts[$0] } ?? ""
        guard let agentID else { return }
        loading = true
        defer { if selectionGeneration == token { loading = false } }
        do {
            let s = try await connection()
            let all = try await s.conversationsList()
            try Task.checkCancellation()
            let c: BotsConversation
            if agentID.hasPrefix("room:") {
                guard let existing = all.first(where: { "room:" + $0.id == agentID && $0.kind != "agent_dm" }) else { throw BotsUIError("Room is no longer available") }
                c = existing
            } else if let existing = all.first(where: { $0.kind == "agent_dm" && $0.storageScope == "local_only" && $0.coordinator == agentID }) { c = existing }
            else { c = try await s.conversationsCreate(agentId: agentID) }
            guard selectionGeneration == token, !Task.isCancelled else { return }
            if c.kind != "agent_dm" {
                let members = try await s.roomAgents(conversationId: c.id)
                guard selectionGeneration == token, !Task.isCancelled else { return }
                roomAgents = members
            }
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
                    let json = try await s.conversationDeliveries(conversationId: c.id)
                    let rows = try JSONDecoder().decode([[String]].self, from: Data(json.utf8))
                    guard selectionGeneration == token, !Task.isCancelled else { return }
                    deliveryNotes = Self.deliveryNote(rows)
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
        do {
            let s = try await connection()
            let d: BotsSend
            if let pending = retry[agentID], pending.body == body, pending.conversationId == c.id { d = pending }
            else {
                var recipients = [agentID]
                if c.kind != "agent_dm" {
                    let resolved = try await s.mentionsResolve(conversationId: c.id, body: body)
                    guard connectionToken == generation, selectionGeneration == token else { return }
                    recipients = resolved.recipientIds
                    mentionNote = resolved.unresolved.isEmpty ? nil : "Not notified: " + resolved.unresolved.map { "@" + $0 }.joined(separator: ", ")
                }
                d = BotsSend(conversationId: c.id, clientRequestId: UUID().uuidString, expectedPolicyRevision: c.policyRevision, recipientIds: recipients, body: body, threadRoot: nil)
                retry[agentID] = d
            }
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
