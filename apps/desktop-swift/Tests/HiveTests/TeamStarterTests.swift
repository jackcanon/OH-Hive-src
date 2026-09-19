import XCTest
@testable import Hive

@MainActor
final class TeamStarterTests: XCTestCase {
    private func defaults() -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("team-tests-" + UUID().uuidString)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    func testSuggestedTeamIsOptionalAndRecipesHaveNoCreationIdentity() throws {
        let store = defaults()
        let setup = TeamStarter(context: "fleet", directory: store)
        XCTAssertTrue(setup.members.isEmpty)
        setup.choose(AgentRoleTemplate.all.filter { $0.id != "assistant-v1" })
        XCTAssertEqual(setup.members.count, 9)
        setup.members[0].included = false
        setup.members[1].name = "My librarian"
        setup.members[1].agentID = "existing"
        try setup.saveRecipe(name: "My team")
        let recipe = try XCTUnwrap(setup.recipes.first)
        XCTAssertEqual(recipe.members.count, 8)
        XCTAssertTrue(recipe.members.allSatisfy { $0.agentID == nil && !$0.creationPending && !$0.complete })
        let other = TeamStarter(context: "another-fleet", directory: store)
        other.use(recipe)
        XCTAssertEqual(other.members.first?.name, "My librarian")
        XCTAssertNotEqual(other.members.first?.id, recipe.members.first?.id)
    }

    func testPartialProfileFailureResumesWithoutDuplicatingAgents() async {
        let store = defaults()
        let setup = TeamStarter(context: "fleet", directory: store)
        setup.choose(Array(AgentRoleTemplate.all.prefix(2)))
        var created = 0
        await setup.create(createAgent: { _ in created += 1; return "agent-\(created)" }, saveProfile: { _, _ in throw TeamStarterError("offline") })
        XCTAssertEqual(created, 1)
        let resumed = TeamStarter(context: "fleet", directory: store)
        var saved: [String] = []
        await resumed.create(createAgent: { _ in created += 1; return "agent-\(created)" }, saveProfile: { id, _ in saved.append(id) })
        XCTAssertEqual(created, 2)
        XCTAssertEqual(saved, ["agent-1", "agent-2"])
        XCTAssertTrue(resumed.members.allSatisfy(\.complete))
    }

    func testLostCreationResponseIsNeverRepeatedAfterRestart() async {
        let store = defaults()
        let setup = TeamStarter(context: "fleet", directory: store)
        setup.choose(Array(AgentRoleTemplate.all.prefix(1)))
        var calls = 0
        await setup.create(createAgent: { _ in calls += 1; throw TeamStarterError("response lost") }, saveProfile: { _, _ in XCTFail() })
        let resumed = TeamStarter(context: "fleet", directory: store)
        XCTAssertFalse(resumed.canCreate)
        await resumed.create(createAgent: { _ in calls += 1; return "duplicate" }, saveProfile: { _, _ in XCTFail() })
        XCTAssertEqual(calls, 1)
        XCTAssertNotNil(resumed.error)
        XCTAssertTrue(TeamStarter(context: "other-fleet", directory: store).members.isEmpty)
    }

    func testConcurrentSetupCannotCreateTheSameTeamTwice() async {
        let directory = defaults()
        let first = TeamStarter(context: "fleet", directory: directory)
        let second = TeamStarter(context: "fleet", directory: directory)
        first.choose(Array(AgentRoleTemplate.all.prefix(1)))
        second.choose(Array(AgentRoleTemplate.all.prefix(1)))
        await first.create(createAgent: { _ in
            await second.create(createAgent: { _ in XCTFail("Concurrent creation"); return "duplicate" }, saveProfile: { _, _ in XCTFail() })
            return "first"
        }, saveProfile: { _, _ in })
        XCTAssertNotNil(second.error)
        await second.create(createAgent: { _ in XCTFail("Stale sheet creation"); return "duplicate" }, saveProfile: { _, _ in XCTFail() })
        XCTAssertTrue(second.members.allSatisfy(\.complete))
    }

    func testInvalidProfilePreventsAnyCreation() async {
        let setup = TeamStarter(context: "fleet", directory: defaults())
        setup.choose(Array(AgentRoleTemplate.all.prefix(1)))
        setup.members[0].name = " "
        XCTAssertFalse(setup.canCreate)
        await setup.create(createAgent: { _ in XCTFail(); return "bad" }, saveProfile: { _, _ in XCTFail() })
    }
}
