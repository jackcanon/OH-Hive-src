import XCTest
import OHHiveFFI
@testable import Hive

final class PrivateReviewSuggestionsTests: XCTestCase {
    private func host(_ id: String, fresh: Bool = true, models: [PrivateCodingModel] = [PrivateCodingModel(id: "task-model", supportsTools: true)]) -> PrivateCodingHost {
        PrivateCodingHost(nodeId: id, name: "Same display name", fresh: fresh, observedAt: nil, workerEnabled: true, codingEnabled: true, gitConnected: true, models: models)
    }
    private func agent(_ id: String, host: String, archived: Bool = false, runtime: String = "local") -> BotsAgent {
        BotsAgent(id: id, owner: "owner", name: "Same display name", runtimeKind: runtime, preferredHost: host, hostName: nil, roleRevision: 1, capabilityPolicyRef: "policy", memoryNamespace: id, archived: archived)
    }
    func testSourceIdentityWinsOverIdenticalDisplayNamesAndInputOrder() {
        let agents = [agent("other-checker", host: "other"), agent("coder", host: "source"), agent("checker", host: "source")]
        let result = PrivateReviewSuggestions.suggest(coder: "coder", model: "task-model", hosts: [host("other"), host("source")], agents: agents)
        XCTAssertEqual(result.host, "source")
        XCTAssertEqual(result.agent, "checker")
        XCTAssertEqual(result.model, "task-model")
    }
    func testSelfArchivedAndCloudAgentsAreNeverSuggestedAsChecker() {
        let agents = [agent("coder", host: "source"), agent("archived", host: "source", archived: true), agent("cloud", host: "source", runtime: "cloud")]
        let result = PrivateReviewSuggestions.suggest(coder: "coder", model: "task-model", hosts: [host("source")], agents: agents)
        XCTAssertTrue(result.host.isEmpty)
        XCTAssertTrue(result.agent.isEmpty)
    }
    func testAmbiguousReviewersRequireAnExplicitChoice() {
        let agents = [agent("coder", host: "source"), agent("first", host: "source"), agent("second", host: "source")]
        let result = PrivateReviewSuggestions.suggest(coder: "coder", model: "task-model", hosts: [host("source")], agents: agents)
        XCTAssertEqual(result.host, "source")
        XCTAssertTrue(result.agent.isEmpty)
    }
    func testUnavailablePreferredHostDoesNotSilentlyChooseAmongOtherHosts() {
        let agents = [agent("coder", host: "source"), agent("first", host: "a"), agent("second", host: "b")]
        let result = PrivateReviewSuggestions.suggest(coder: "coder", model: nil, hosts: [host("source", fresh: false), host("a"), host("b")], agents: agents)
        XCTAssertTrue(result.host.isEmpty)
    }
    func testUniqueAvailableAlternativeCanBeSuggested() {
        let agents = [agent("coder", host: "source"), agent("checker", host: "a")]
        let result = PrivateReviewSuggestions.suggest(coder: "coder", model: nil, hosts: [host("source", fresh: false), host("a")], agents: agents)
        XCTAssertEqual(result.host, "a")
        XCTAssertEqual(result.agent, "checker")
    }
    func testUnsupportedSavedModelAndMultipleAlternativesRequireChoice() {
        let models = [PrivateCodingModel(id: "task-model", supportsTools: false), PrivateCodingModel(id: "a", supportsTools: true), PrivateCodingModel(id: "b", supportsTools: true)]
        let result = PrivateReviewSuggestions.onHost(host("source", models: models), coder: "coder", model: "task-model", agents: [agent("checker", host: "source")])
        XCTAssertTrue(result.model.isEmpty)
    }
}
