import XCTest
import OHHiveFFI
@testable import Hive

final class PrivateCodingTaskPresentationTests: XCTestCase {
    private func task(preparation: String? = nil, reason: String? = nil, run: String? = nil, status: String = "blocked") -> RemoteCodingTask {
        RemoteCodingTask(modelId: "model", codingThink: nil, savedFiles: [], id: "task", target: "worker", title: "task", agentName: "coder", agentId: "coder", reviewAvailable: false, reviewSourceTaskId: nil, checkerVerdict: nil, status: status, reason: reason, output: nil, checkCount: 1, preparationId: "preparation", preparationState: preparation, runId: run == nil ? nil : "attempt", runState: run, leaseActive: run == "running")
    }

    func testStoppedWorkflowRetainsPreparationInHistoryWithoutClaimingItIsWaiting() {
        for preparation in ["queued", "claimed", "prepared"] {
            let job = task(preparation: preparation, reason: "review_workflow_stopped")
            XCTAssertFalse(PrivateCodingTaskPresentation.isCurrent(job, workflowActive: false))
            XCTAssertEqual(PrivateCodingTaskPresentation.statusLabel(job), "Workflow stopped")
        }
    }

    func testExplicitRetryStillShowsCurrentExecutionAfterWorkflowStopped() {
        for state in ["queued", "running", "stopping"] {
            let job = task(preparation: "prepared", reason: "review_workflow_stopped", run: state)
            XCTAssertTrue(PrivateCodingTaskPresentation.isCurrent(job, workflowActive: false))
            XCTAssertEqual(PrivateCodingTaskPresentation.statusLabel(job), state.capitalized)
        }
    }

    func testOrdinaryPreparationAndActiveWorkflowRemainCurrent() {
        let waiting = task(preparation: "queued", reason: "awaiting_repository_preparation")
        XCTAssertTrue(PrivateCodingTaskPresentation.isCurrent(waiting, workflowActive: false))
        XCTAssertEqual(PrivateCodingTaskPresentation.statusLabel(waiting), "Waiting for worker")
        XCTAssertTrue(PrivateCodingTaskPresentation.isCurrent(task(run: "finished", status: "review"), workflowActive: true))
    }

    func testFinishedAndStoppedAttemptsStayInHistory() {
        for state in ["finished", "stopped", "blocked", "interrupted", "superseded"] {
            XCTAssertFalse(PrivateCodingTaskPresentation.isCurrent(task(preparation: "prepared", run: state), workflowActive: false))
        }
        XCTAssertEqual(PrivateCodingTaskPresentation.statusLabel(task(run: "finished", status: "review")), "Ready for review")
    }
}
