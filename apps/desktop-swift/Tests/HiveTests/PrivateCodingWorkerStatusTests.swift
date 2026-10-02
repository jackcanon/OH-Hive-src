import XCTest
@testable import Hive

@MainActor
final class PrivateCodingWorkerStatusTests: XCTestCase {
    func testMissingGitHubExplainsPreparationRequirement() {
        let status = PrivateCodingWorkerModel.idleStatus(gitConnected: false)
        XCTAssertTrue(status.contains("Connect GitHub"))
        XCTAssertTrue(status.contains("Prepared tasks can still run"))
        XCTAssertFalse(status.hasPrefix("Ready."))
        XCTAssertEqual(PrivateCodingWorkerModel.idleStatus(gitConnected: true), "Ready. Waiting for a task from your primary.")
    }
}
