import XCTest
@testable import Hive

final class AppTerminationCoordinatorTests: XCTestCase {
    @MainActor
    func testWaitsForCleanupAndCoalescesRepeatedQuitRequests() async {
        let coordinator = AppTerminationCoordinator()
        let started = expectation(description: "cleanup started")
        let replied = expectation(description: "termination replied")
        var resume: CheckedContinuation<Bool, Never>?
        var replies = 0
        coordinator.request(shutdown: {
            started.fulfill()
            return await withCheckedContinuation { resume = $0 }
        }, reply: { safeToExit in
            XCTAssertTrue(safeToExit)
            replies += 1
            replied.fulfill()
        })
        await fulfillment(of: [started], timeout: 2)
        coordinator.request(shutdown: {
            XCTFail("must not start cleanup twice")
            return true
        }, reply: { _ in XCTFail("must reply once") })
        XCTAssertEqual(replies, 0, "must not exit while commands are still draining")
        resume?.resume(returning: true)
        await fulfillment(of: [replied], timeout: 2)
        XCTAssertEqual(replies, 1)
    }

    @MainActor
    func testFailedCleanupCancelsTerminationAndAllowsAnotherAttempt() async {
        let coordinator = AppTerminationCoordinator()
        let refused = expectation(description: "unsafe exit refused")
        coordinator.request(shutdown: { false }, reply: { safeToExit in
            XCTAssertFalse(safeToExit)
            refused.fulfill()
        })
        await fulfillment(of: [refused], timeout: 2)
        let retried = expectation(description: "retry accepted")
        coordinator.request(shutdown: { true }, reply: { safeToExit in
            XCTAssertTrue(safeToExit)
            retried.fulfill()
        })
        await fulfillment(of: [retried], timeout: 2)
    }
}
