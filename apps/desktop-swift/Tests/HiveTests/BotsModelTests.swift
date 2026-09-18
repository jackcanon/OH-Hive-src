import XCTest
import AppKit
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
    var remote = false
    var deleted = false
    override func hostAgentDeleted() async throws -> Bool { deleted }
    override func agentBioGet(agentId: String) async throws -> String { "{\"bio\":\"\",\"instructions\":\"\",\"avatar\":\"sif\",\"revision\":0}" }
    override func agentsArchive(agentId: String) async throws { hasHostAgent = false; deleted = true }
    var hasHostAgent = true
    var createdCount = 0
    override func usesRemotePrimary() -> Bool { remote }
    override func agentsCreate(name: String) async throws -> BotsAgent {
        createdCount += 1; hasHostAgent = true
        return try await agentsList()[0]
    }
    override func hostId() -> String { "host" }
    override func ensureProviderAgents() async throws -> [BotsAgent] { [] }
    override func agentsList() async throws -> [BotsAgent] {
        guard hasHostAgent else { return [] }
        return [BotsAgent(id: "agent", owner: "owner", name: "Midgaard", runtimeKind: "local", preferredHost: "host", hostName: "Midgaard", roleRevision: 1, capabilityPolicyRef: "default", memoryNamespace: "agent", archived: false)]
    }
    override func agentsUpdate(agentId: String, name: String?, capabilityPolicyRef: String?) async throws -> BotsAgent {
        BotsAgent(id: agentId, owner: "owner", name: name ?? "Midgaard", runtimeKind: "local", preferredHost: "host", hostName: "Midgaard", roleRevision: 1, capabilityPolicyRef: capabilityPolicyRef ?? "default", memoryNamespace: "agent", archived: false)
    }
    override func conversationsList() async throws -> [BotsConversation] { [convo, room] }
    override func roomAgents(conversationId: String) async throws -> [BotsAgent] { try await agentsList() }
    override func mentionsResolve(conversationId: String, body: String) async throws -> BotsMentions {
        BotsMentions(recipientIds: body.contains("@Midgaard") ? ["agent"] : [], unresolved: body.contains("@typo") ? ["typo"] : [])
    }
    var roomRequests: [String] = []
    var failRoomOnce = false
    override func roomsCreate(requestId: String, title: String, kind: String, agentIds: [String], projectId: String?, coordinatorId: String?) async throws -> BotsConversation {
        roomRequests.append(requestId)
        if failRoomOnce { failRoomOnce = false; throw NSError(domain: "lost response", code: 1) }
        return room
    }
    override func conversationDeliveries(conversationId: String) async throws -> String { "[]" }
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
    func testAllAvatarResourcesDecode() throws {
        for avatar in AgentAvatar.choices {
            let url = try XCTUnwrap(Bundle.module.url(forResource: avatar + "-128", withExtension: "gif", subdirectory: "Avatars"))
            XCTAssertNotNil(NSImage(contentsOf: url), avatar)
        }
    }
    func testDeletedHostDoesNotReappearAfterRefreshOrReconnect() async throws {
        let fake = FakeBots(); fake.remote = true
        let model = BotsModel(openSession: { fake }); model.setPaired(true)
        defer { model.setPaired(false) }
        await model.refreshAgents()
        model.selectedID = "agent"
        try await model.deleteAgent("agent")
        await model.reconnect()
        XCTAssertTrue(model.agents.isEmpty)
        XCTAssertEqual(fake.createdCount, 0)
    }
    func testReplyFailureStaysAttachedToItsMessage() {
        let notes = BotsModel.deliveryNote([["failed-id", "Overgaard", "failed"], ["ok-id", "Overgaard", "done"]])
        XCTAssertTrue(notes["failed-id"]!.contains("Reply failed"))
        XCTAssertEqual(notes["ok-id"], "Overgaard: Replied")
    }

    func testSecondaryRegistersItsAuthenticatedHostOnceAcrossReconnects() async {
        let fake = FakeBots(); fake.remote = true; fake.hasHostAgent = false
        let model = BotsModel(openSession: { fake }); model.setPaired(true)
        defer { model.setPaired(false) }
        await model.refreshAgents()
        XCTAssertEqual(fake.createdCount, 1)
        await model.reconnect()
        XCTAssertEqual(fake.createdCount, 1)
    }

    func testPrimaryDoesNotAutomaticallyCreateLocalAgent() async {
        let fake = FakeBots(); fake.hasHostAgent = false
        let model = BotsModel(openSession: { fake }); model.setPaired(true)
        defer { model.setPaired(false) }
        await model.refreshAgents()
        XCTAssertEqual(fake.createdCount, 0)
    }

    func testRemoteLocalAgentCanBeMessagedWithoutLocalHostMatch() {
        let agent = BotsAgent(id: "remote", owner: "owner", name: "Overgaard", runtimeKind: "local", preferredHost: "other-host", hostName: "Niflheim", roleRevision: 1, capabilityPolicyRef: "default", memoryNamespace: "remote", archived: false)
        XCTAssertTrue(BotsModel.canMessageAgent(agent))
        XCTAssertFalse(BotsModel.canMessageAgent(nil))
        let unassigned = BotsAgent(id: "unassigned", owner: "owner", name: "Unassigned", runtimeKind: "local", preferredHost: nil, hostName: nil, roleRevision: 1, capabilityPolicyRef: "default", memoryNamespace: "unassigned", archived: false)
        XCTAssertFalse(BotsModel.canMessageAgent(unassigned))
    }

    func testRoomCreationRetriesSameRequestAndDeduplicatesList() async throws {
        let fake = FakeBots(); fake.failRoomOnce = true
        let model = BotsModel(openSession: { fake }); model.setPaired(true)
        defer { model.setPaired(false) }
        await model.refreshAgents()
        do { try await model.createRoom(title: "Launch", agentIDs: ["agent"], projectID: "project", coordinatorID: nil); XCTFail("Expected lost response") } catch {}
        await model.reconnect()
        try await model.createRoom(title: "Launch", agentIDs: ["agent"], projectID: "project", coordinatorID: nil)
        XCTAssertEqual(fake.roomRequests.count, 2)
        XCTAssertEqual(fake.roomRequests[0], fake.roomRequests[1])
        XCTAssertEqual(model.rooms.filter { $0.id == "room" }.count, 1)
        try await model.createRoom(title: "Launch", agentIDs: ["agent"], projectID: "project", coordinatorID: nil)
        XCTAssertNotEqual(fake.roomRequests[1], fake.roomRequests[2])
    }

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
