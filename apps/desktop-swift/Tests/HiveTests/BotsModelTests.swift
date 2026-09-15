import XCTest
import OHHiveFFI
@testable import Hive

private final class FakeBots: BotsSession, @unchecked Sendable {
    private let lock = NSLock()
    private var requests: [BotsSend] = []
    var sent: [BotsSend] { lock.withLock { requests } }
    let convo = BotsConversation(id: "dm", owner: "owner", kind: "agent_dm", projectId: nil, coordinator: "agent", storageScope: "local_only", policyRevision: 1, createdAt: "2026-09-15T19:00:00Z")
    init() { super.init(noPointer: .init()) }
    required init(unsafeFromRawPointer pointer: UnsafeMutableRawPointer) { super.init(unsafeFromRawPointer: pointer) }
    override func hostId() -> String { "host" }
    override func agentsList() async throws -> [BotsAgent] {
        [BotsAgent(id: "agent", owner: "owner", name: "Midgaard", runtimeKind: "local", preferredHost: "host", roleRevision: 1, capabilityPolicyRef: "default", memoryNamespace: "agent", archived: false)]
    }
    override func conversationsList() async throws -> [BotsConversation] { [convo] }
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
