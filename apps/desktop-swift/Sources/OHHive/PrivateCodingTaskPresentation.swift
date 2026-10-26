import OHHiveFFI

/// Present durable task state without interpreting a retained preparation as new work.
enum PrivateCodingTaskPresentation {
    static func isCurrent(_ job: RemoteCodingTask, workflowActive: Bool) -> Bool {
        if workflowActive { return true }
        if let state = job.runState { return ["queued", "running", "stopping"].contains(state) }
        if job.reason == "review_workflow_stopped" { return false }
        return job.runId == nil && (job.reason == "awaiting_repository_preparation" || ["queued", "claimed", "prepared"].contains(job.preparationState ?? ""))
    }

    static func statusLabel(_ job: RemoteCodingTask) -> String {
        if let verdict = job.checkerVerdict {
            switch verdict {
            case "pass": return "Snapshot check passed"
            case "changes_required": return "Changes needed"
            default: return "Check inconclusive"
            }
        }
        if job.status == "review" { return "Ready for review" }
        if let state = job.runState { return state.capitalized }
        if job.reason == "review_workflow_stopped" { return "Workflow stopped" }
        if job.preparationState == "prepared" { return "Ready to run" }
        if job.preparationState == "claimed" { return "Preparing on \(job.target)" }
        if job.preparationState == "queued" { return "Waiting for \(job.target)" }
        return "Needs preparation"
    }
}
