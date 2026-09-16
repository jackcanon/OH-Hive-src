import XCTest
import OHHiveFFI
@testable import Hive

private final class FakeBots: BotsSession, @unchecked Sendable {
    private let lock = NSLock()
    private var requests: [BotsSend] = []
    var sent: [BotsSend] { lock.withLock { requests } }
    let room = BotsConversation(title: "Launch", id: "room", owner: "owner", kind: "project", projectId: "project", coordinator: nil, storageScope: "local_only", policyRevision: 1, createdAt: "2026-09-15T19:00:00Z")
    let convo = BotsConversation(title: nil, id: "dm", owner: "owner", kind: "agent_dm", projectId: nil, coordinator: "agent", storageScope: "local_only", policyRevision: 1, createdAt: "2026-09-15T19:00:00Z")
    init() { super.init(noPointer: .init()) }
    required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) { super.init(unsafeFromRawPointer: pointer) }
    override func ownerId() -> String { "owner" }
    override func usesRemotePrimary() -> Bool { false }
    override func hostId() -> String { "host" }
    override func agentsList() async throws -> [BotsAgent] {
        [BotsAgent(id: "agent", owner: "owner", name: "Midgaard", runtimeKind: "local", preferredHost: "host", roleRevision: 1, capabilityPolicyRef: "default", memoryNamespace: "agent", archived: false)]
    }
    override func agentsUpdate(agentId: String, name: String?, capabilityPolicyRef: String?) async throws -> BotsAgent {
        BotsAgent(id: agentId, owner: "owner", name: name ?? "Midgaard", runtimeKind: "local", preferredHost: "host", roleRevision: 1, capabilityPolicyRef: capabilityPolicyRef ?? "default", memoryNamespace: "agent", archived: false)
    }
    override func conversationsList() async throws -> [BotsConversation] { [convo, room] }
    override func roomAgents(conversationId: String) async throws -> [BotsAgent] { try await agentsList() }
    override func mentionsResolve(conversationId: String, body: String) async throws -> BotsMentions {
        BotsMentions(recipientIds: body.contains("@Midgaard") ? ["agent"] : [], unresolved: body.contains("@typo") ? ["typo"] : [])
    }
    override func roomsCreate(title: String, kind: String, agentIds: [String], projectId: String?, coordinatorId: String?) async throws -> BotsConversation { room }
    override func messagesList(conversationId: String, page: BotsPage) async throws -> [BotsMessage] { [] }
    override func drainOnce() async throws -> BotsDrain { BotsDrain(delivered: 0, failed: 0, requeued: 0) }
    override func messageSend(draft: BotsSend) async throws -> BotsMessage {
        let count = lock.withLock { requests.append(draft); return requests.count }
        if count == 1 { throw NSError(domain: "test", code: 1) }
        return BotsMessage(id: "sent", conversationId: "dm", threadRoot: nil, authorKind: "user", authorId: "owner", serverSequence: 3, clientRequestId: draft.clientRequestId, kind: "text", body: draft.body, attachmentRefs: [], taskRef: nil, turnRef: nil, sourceEventRef: nil, createdAt: "2026-09-15T19:00:00Z")
    }
}

@MainActor
final class BotsModelTests: XCTestCase {
    func testRoomRetryKeepsResolvedRecipientsAndProjectSelection() async throws {
        let fake = FakeBots()
        let model = BotsModel(openSession: { fake })
        model.setPaired(true)
        await model.refreshAgents()
        try await model.createRoom(title: "Launch", agentIDs: ["agent"], projectID: "project", coordinatorID: nil)
        XCTAssertEqual(model.selectedID, "room:room")
        let watch = Task { await model.watch(agentID: model.selectedID) }
        defer { watch.cancel(); model.setPaired(false) }
        for _ in 0..<200 { if model.conversation != nil { break }; await Task.yield() }
        XCTAssertEqual(model.conversation?.projectId, "project")
        model.draft = "@Midgaard @typo hello"; model.saveDraft()
        await model.send(); await model.send()
        XCTAssertEqual(fake.sent.count, 2)
        XCTAssertEqual(fake.sent.first?.recipientIds, ["agent"])
        XCTAssertEqual(fake.sent.first?.clientRequestId, fake.sent.last?.clientRequestId)
        XCTAssertEqual(model.mentionNote, "Not notified: @typo")
        model.draft = "quiet post"
        await model.send()
        XCTAssertEqual(fake.sent.last?.recipientIds, [])
    }

    func testInspectorUpdatePreservesSelectionAndDraft() async throws {
        let fake = FakeBots()
        let model = BotsModel(openSession: { fake })
        model.setPaired(true)
        defer { model.setPaired(false) }
        await model.refreshAgents()
        model.selectedID = "agent"; model.draft = "unfinished message"
        try await model.update(agentID: "agent", name: "New name", capabilityPolicyRef: "future-policy")
        XCTAssertEqual(model.agents.first?.name, "New name")
        XCTAssertEqual(model.agents.first?.capabilityPolicyRef, "future-policy")
        XCTAssertEqual(model.selectedID, "agent")
        XCTAssertEqual(model.draft, "unfinished message")
    }

    func testRetryKeepsRequestIDAndReceiptDoesNotSkipHistory() async throws {
        let fake = FakeBots()
        let model = BotsModel(openSession: { fake })
        model.setPaired(true); model.selectedID = "agent"
        let watch = Task { await model.watch(agentID: "agent") }
        defer { watch.cancel(); model.setPaired(false) }
        for _ in 0..<100 {
            if model.conversation != nil { break }
            await Task.yield()
        }
        XCTAssertNotNil(model.conversation)
        model.draft = "hello"
        await model.send()
        XCTAssertEqual(model.draft, "hello")
        XCTAssertNotNil(model.sendError)
        await model.send()
        XCTAssertEqual(fake.sent.count, 2)
        XCTAssertEqual(fake.sent.first?.clientRequestId, fake.sent.last?.clientRequestId)
        XCTAssertEqual(model.draft, "")
        XCTAssertTrue(model.messages.isEmpty, "Send receipt must not advance polling past unseen messages")
    }

    func testReconnectPreservesPendingRequestAndDraft() async throws {
        let fake = FakeBots()
        let model = BotsModel(openSession: { fake })
        model.setPaired(true); model.selectedID = "agent"
        let watch = Task { await model.watch(agentID: "agent") }
        for _ in 0..<100 { if model.conversation != nil { break }; await Task.yield() }
        model.draft = "pending across reconnect"; model.saveDraft()
        await model.send()
        let request = fake.sent.first?.clientRequestId
        XCTAssertNotNil(request)
        watch.cancel(); await watch.value
        await model.reconnect()
        XCTAssertEqual(model.draft, "pending across reconnect")
        let resumed = Task { await model.watch(agentID: "agent") }
        defer { resumed.cancel(); model.setPaired(false) }
        for _ in 0..<100 { if model.conversation != nil { break }; await Task.yield() }
        await model.send()
        XCTAssertEqual(fake.sent.count, 2)
        XCTAssertEqual(fake.sent.last?.clientRequestId, request)
    }

    func testChangingPrimaryClearsOldPrivateDrafts() async throws {
        let model = BotsModel(openSession: { FakeBots() })
        model.setPaired(true); model.selectedID = "agent"
        model.draft = "belongs to old primary"; model.saveDraft()
        model.setPrimary("http://192.168.1.10:8787")
        XCTAssertEqual(model.draft, "")
        XCTAssertNil(model.ownerID)
        XCTAssertTrue(model.messages.isEmpty)
        model.setPaired(false)
    }

    func testUnpairClearsPrivateStateAndInvalidatesInFlightOpen() async throws {
        let fake = FakeBots()
        var continuation: CheckedContinuation<BotsSession, Never>?
        let model = BotsModel(openSession: { await withCheckedContinuation { continuation = $0 } })
        model.setPaired(true)
        let refresh = Task { await model.refreshAgents() }
        for _ in 0..<100 { if continuation != nil { break }; await Task.yield() }
        XCTAssertNotNil(continuation)
        model.draft = "private draft"
        model.setPaired(false)
        continuation?.resume(returning: fake)
        await refresh.value
        XCTAssertNil(model.hostID)
        XCTAssertTrue(model.agents.isEmpty)
        XCTAssertEqual(model.draft, "")
        XCTAssertFalse(model.paired)
    }
}
