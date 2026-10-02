import SwiftUI
import OHHiveFFI

struct PrivateCodingTasksView: View {
    @EnvironmentObject private var store: HiveStore
    @Environment(\.dismiss) private var dismiss
    let project: PrivateRepositoryProject
    @State private var showingNewTask = false
    @State private var workflows: [CodingReviewWorkflow] = []
    @State private var workflowRequests: [String: String] = [:]
    @State private var correctionLimit = 1
    @State private var jobs: [RemoteCodingTask] = []
    @State private var agents: [BotsAgent] = []
    @State private var agent = ""
    @State private var hosts: [PrivateCodingHost] = []
    @State private var target = ""
    @State private var model = ""
    @State private var requestID = UUID().uuidString
    @State private var title = ""
    @State private var instructions = ""
    @State private var turns = 6
    @State private var reasoningOff = false
    @State private var allowVerification = false
    @State private var checks: [TaskCheckDraft] = []
    @State private var busy = false
    @State private var error: String?
    @State private var message: String?
    @State private var reviewSource: RemoteCodingTask?
    @State private var checkerTarget = ""
    @State private var checkerAgent = ""
    @State private var checkerModel = ""
    @State private var checkerReasoningOff = false
    @State private var reviewRequests: [String: String] = [:]
    @State private var correctionRequests: [String: String] = [:]
    @State private var confirmation: PendingCommand?
    @State private var commandIDs: [String: String] = [:]

    private struct PendingCommand {
        let job: RemoteCodingTask
        let action: String
    }
    private var selectedHost: PrivateCodingHost? { hosts.first { $0.nodeId == target } }
    private var availableAgents: [BotsAgent] { agents.filter { $0.preferredHost == target && !$0.archived && $0.runtimeKind == "local" } }
    private var agentAvailable: Bool { agent.isEmpty || availableAgents.contains { $0.id == agent } }
    private var models: [PrivateCodingModel] { selectedHost?.models.filter { $0.supportsTools != false } ?? [] }
    private var hostReady: Bool {
        guard let host = selectedHost else { return false }
        return host.fresh && host.workerEnabled && host.codingEnabled
    }

    private var checkerHost: PrivateCodingHost? { hosts.first { $0.nodeId == checkerTarget } }
    private var checkerAgents: [BotsAgent] { agents.filter { $0.preferredHost == checkerTarget && !$0.archived && $0.runtimeKind == "local" && $0.id != reviewSource?.agentId } }
    private var checkerModels: [PrivateCodingModel] { checkerHost?.models.filter { $0.supportsTools != false } ?? [] }
    private var checkerReady: Bool { checkerHost.map { $0.fresh && $0.workerEnabled && $0.codingEnabled } ?? false }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Tasks · \(project.title)").font(.title2)
                Spacer()
                Button("Close") { dismiss() }
            }
            Text("Choose where each task runs. This primary keeps the history; the execution computer uses its own model and GitHub connection.")
                .font(.caption).foregroundStyle(.secondary)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    DisclosureGroup("New task", isExpanded: $showingNewTask) { newTask }
                    if let source = reviewSource { checkerSetup(source) }
                    if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
                    if let message { Text(message).font(.caption).foregroundStyle(.secondary) }
                    HStack {
                        if busy { ProgressView().controlSize(.small) }
                        Spacer()
                        Button("Refresh computers and tasks") { Task { await refresh() } }
                    }
                    Text("Agent work").font(.headline)
                    Text("Live status refreshes every five seconds. Open a task’s result to inspect its saved evidence.").font(.caption).foregroundStyle(.secondary)
                    ForEach(workflows, id: \.id) { workflow in workflowRow(workflow) }
                    ForEach(jobs, id: \.id) { job in taskRow(job) }
                    if jobs.isEmpty { Text("No tasks yet. Open New task to assign work.").foregroundStyle(.secondary) }
                }
            }
        }
        .textFieldStyle(.roundedBorder)
        .padding(24).frame(width: 760, height: 650)
        .confirmationDialog(confirmation?.action == "workflow" ? "Run with independent review?" : confirmation?.action == "correct" ? "Create a correction task?" : confirmation?.action == "recover" ? "Recover preparation?" : "Start another attempt?", isPresented: Binding(get: { confirmation != nil }, set: { if !$0 { confirmation = nil } }), titleVisibility: .visible) {
            if let pending = confirmation {
                Button(pending.action == "workflow" ? "Start workflow" : pending.action == "correct" ? "Save correction task" : pending.action == "recover" ? "Recover preparation" : "Keep files and run again") {
                    confirmation = nil
                    Task {
                        if pending.action == "workflow" { await startWorkflow(pending.job) }
                        else if pending.action == "correct" { await stageCorrection(pending.job) }
                        else { await command(pending.action, pending.job) }
                    }
                }
            }
            Button("Cancel", role: .cancel) { confirmation = nil }
        } message: {
            Text(confirmation?.action == "workflow"
                ? "This authorizes preparation, coding, independent review and up to \(correctionLimit) correction rounds. It uses the saved task settings and selected checker. It stops after 30 minutes, on uncertainty, failure or interruption. Keep the execution apps and workers open. You can stop it here. Nothing is merged or published."
                : confirmation?.action == "correct"
                ? "The original agent receives the saved code and findings in a fresh checkout. Its computer, model, instructions and checks stay the same. Earlier work is kept. Prepare and run the new task next. Each correction chain is limited to three rounds."
                : confirmation?.action == "recover"
                ? "Use this after the previous worker stopped. Existing files are kept. Restart the worker on the execution computer after recovery. This does not run a model."
                : "The agent will run again on the same computer with the existing files and checks. Earlier actions may be repeated. An active attempt cannot be retried.")
        }
        .onChange(of: target) { _, _ in model = ""; agent = "" }
        .onChange(of: checkerTarget) { _, _ in checkerAgent = ""; checkerModel = "" }
        .task {
            while !Task.isCancelled {
                await refresh()
                do { try await Task.sleep(for: .seconds(5)) } catch { break }
            }
        }
    }
    private var newTask: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                TextField("Task title", text: $title)
                TextField("What should the agent do?", text: $instructions, axis: .vertical).lineLimit(3...6)
                Picker("Run on", selection: $target) {
                    Text("Choose a computer").tag("")
                    ForEach(hosts, id: \.nodeId) { host in
                        Text(host.name + (host.fresh && host.workerEnabled && host.codingEnabled ? " · ready" : " · unavailable")).tag(host.nodeId)
                    }
                }
                if !target.isEmpty {
                    Picker("Agent", selection: $agent) {
                        Text("Use task instructions only").tag("")
                        ForEach(availableAgents, id: \.id) { choice in Text(choice.name).tag(choice.id) }
                    }
                    if !agentAvailable {
                        Text("This agent is no longer available on the selected computer. Choose another agent.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Text("The selected agent uses its saved instructions. Run allows it to edit files and execute commands in this task’s checkout.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if hostReady {
                    Picker("Model", selection: $model) {
                        Text("Choose a model").tag("")
                        ForEach(models, id: \.id) { choice in
                            Text(choice.id + (choice.supportsTools == nil ? " (tools unconfirmed)" : "")).tag(choice.id)
                        }
                    }
                    if selectedHost?.gitConnected == false {
                        Text("For private repositories, connect GitHub on the execution computer.").font(.caption).foregroundStyle(.secondary)
                    }
                } else {
                    Text("On the execution computer, connect to this primary and start its coding worker in Private Fleet settings. Computers with no recent report stay unavailable.").font(.caption).foregroundStyle(.secondary)
                }
                DisclosureGroup("Checks and options") {
                    Picker("Reasoning", selection: $reasoningOff) {
                        Text("Model default").tag(false)
                        Text("Reasoning off").tag(true)
                    }
                    Text("Use the model’s default, or turn reasoning off for this coding task. Reasoning off passed our small Helheim coding checks with qwen3.6:35b-a3b; larger tasks still need testing.")
                        .font(.caption).foregroundStyle(.secondary)
                    Stepper("Maximum turns: \(turns)", value: $turns, in: 1...20)
                    TaskChecksEditor(checks: $checks, repository: project.repoUrl, reference: project.repoRef, task: instructions)
                    Toggle("Let the agent run selected checks", isOn: $allowVerification)
                        .disabled(checks.isEmpty)
                    Text("Allows one check run while the agent works. Checks may change files. Final checks still run after its report.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                HStack {
                    Text(checks.isEmpty ? "No checks selected · results will be unverified." : "\(checks.count) required checks").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Save task") { Task { await stage() } }
                        .disabled(!agentAvailable || !hostReady || !models.contains { $0.id == model } || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || instructions.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || checks.contains { !$0.isValid })
                }
            }.disabled(busy)
        }
    }
    private func taskRow(_ job: RemoteCodingTask) -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text(job.title).font(.headline)
                    Spacer()
                    Text(statusLabel(job)).font(.caption)
                }
                if let name = job.agentName { Text("Agent: \(name)").font(.caption).foregroundStyle(.secondary) }
                Text("Execution computer: \(job.target)").font(.caption).foregroundStyle(.secondary)
                HStack {
                    if job.preparationId == nil && job.reason == "awaiting_repository_preparation" {
                        Button("Prepare on \(job.target)") { Task { await command("prepare", job) } }
                    } else if job.preparationState == "claimed" {
                        Button("Recover interrupted preparation…") { confirmation = PendingCommand(job: job, action: "recover") }
                    }
                    if job.preparationState == "prepared" && job.runId == nil {
                        Button("Run on \(job.target)") { Task { await command("run", job) } }
                    }
                    if let state = job.runState, ["queued", "running", "stopping"].contains(state) {
                        Button("Stop task") { Task { await command("stop", job) } }
                    }
                    if let state = job.runState, ["blocked", "interrupted", "stopped"].contains(state), !job.leaseActive {
                        Button("Retry on \(job.target)…") { confirmation = PendingCommand(job: job, action: "retry") }
                    }
                }.disabled(busy || workflowActive(job))
                if job.reviewAvailable && !workflowActive(job) {
                    Button("Request independent check…") { reviewSource = job; checkerAgent = "" }
                        .disabled(busy)
                }
                if job.agentId != nil && job.reviewSourceTaskId == nil && job.checkCount > 0 && job.runId == nil && !workflowActive(job) {
                    Button("Run with independent review…") { reviewSource = job; checkerAgent = "" }
                        .disabled(busy)
                }
                if job.checkerVerdict == "changes_required" && !workflowActive(job) && ["review", "done"].contains(job.status) {
                    Button("Request correction…") { confirmation = PendingCommand(job: job, action: "correct") }
                        .disabled(busy)
                }
                if let source = job.reviewSourceTaskId {
                    Text("Checks a saved copy of task \(jobs.first { $0.id == source }?.title ?? source). The checker cannot edit files or run commands.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Text(job.reviewSourceTaskId != nil ? "Source test results are prior evidence; the checker does not rerun them." : job.checkCount == 0 ? "No acceptance checks · unverified" : "\(job.checkCount) required checks").font(.caption).foregroundStyle(.secondary)
                if let reason = job.reason, !["awaiting_repository_preparation", "awaiting_private_run", "awaiting_retry_validation"].contains(reason) {
                    Text(reason.replacingOccurrences(of: "_", with: " ")).font(.caption).textSelection(.enabled)
                }
                if !job.savedFiles.isEmpty {
                    DisclosureGroup("Saved files (\(job.savedFiles.count))") {
                        Text("Frozen files from this task’s saved result; the live checkout may have changed.").font(.caption).foregroundStyle(.secondary)
                        ForEach(job.savedFiles, id: \.path) { file in
                            DisclosureGroup(file.path) {
                                Text(file.after ?? "File deleted in this result.")
                                    .font(.system(.caption, design: .monospaced))
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                    }
                }
                if let output = job.output {
                    DisclosureGroup("Result and saved evidence") { Text(output).font(.callout).textSelection(.enabled) }
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    private func checkerSetup(_ source: RemoteCodingTask) -> some View {
        GroupBox("Review setup · \(source.title)") {
            VStack(alignment: .leading, spacing: 8) {
                Text("A different agent reviews the saved files and test results. It cannot edit files, run commands or approve later changes.")
                    .font(.caption).foregroundStyle(.secondary)
                Picker("Computer", selection: $checkerTarget) {
                    Text("Choose a computer").tag("")
                    ForEach(hosts, id: \.nodeId) { Text($0.name).tag($0.nodeId) }
                }
                Picker("Checker", selection: $checkerAgent) {
                    Text("Choose a different agent").tag("")
                    ForEach(checkerAgents, id: \.id) { Text($0.name).tag($0.id) }
                }
                Picker("Model", selection: $checkerModel) {
                    Text("Choose a model").tag("")
                    ForEach(checkerModels, id: \.id) { Text($0.id).tag($0.id) }
                }
                Stepper("Maximum correction rounds: \(correctionLimit)", value: $correctionLimit, in: 0...3)
                Toggle("Turn reasoning off for this check", isOn: $checkerReasoningOff)
                if !checkerReady { Text("Start the coding worker on the selected computer to continue.").font(.caption).foregroundStyle(.secondary) }
                HStack {
                    Button("Cancel") { reviewSource = nil }
                    Spacer()
                    if source.reviewAvailable { Button("Save checker task") { Task { await stageChecker(source) } }
                        .disabled(busy || !checkerReady || !checkerAgents.contains { $0.id == checkerAgent } || !checkerModels.contains { $0.id == checkerModel }) }
                }
                Button("Run with independent review…") { confirmation = PendingCommand(job: source, action: "workflow") }
                    .disabled(busy || !checkerReady || !checkerAgents.contains { $0.id == checkerAgent } || !checkerModels.contains { $0.id == checkerModel } || source.checkCount == 0 || workflows.contains { $0.source == source.id })
            }
        }
    }
    private func stageChecker(_ source: RemoteCodingTask) async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        let key = "\(source.id)/\(checkerTarget)/\(checkerAgent)/\(checkerModel)/\(checkerReasoningOff)"
        let request = reviewRequests[key] ?? UUID().uuidString
        reviewRequests[key] = request
        do {
            try await store.stageIndependentCheck(request: request, project: project.id, target: checkerTarget, source: source.id, agent: checkerAgent, model: checkerModel, codingThink: checkerReasoningOff ? false : nil)
            reviewSource = nil
            message = "Checker task saved. Prepare and run it below; no repository download is needed."
            await refresh()
        } catch { self.error = readableError(error) }
    }
    private func workflowActive(_ job: RemoteCodingTask) -> Bool {
        workflows.contains { $0.state == "active" && ($0.source == job.id || $0.current == job.id) }
    }
    private func workflowRow(_ workflow: CodingReviewWorkflow) -> some View {
        GroupBox("Coding and review · \(jobs.first { $0.id == workflow.source }?.title ?? workflow.source)") {
            VStack(alignment: .leading, spacing: 8) {
                Text(workflow.state == "passed" ? "Independent review passed" : workflow.state.capitalized).font(.headline)
                let coder = jobs.first { $0.id == workflow.coderTask }
                let reviewer = workflow.checking ? jobs.first { $0.id == workflow.current } : nil
                HStack(alignment: .top, spacing: 16) {
                    workflowStage("Coder", name: coder?.agentName ?? "Assigned agent", detail: coder.map(statusLabel) ?? "Waiting", icon: "hammer")
                    Image(systemName: "arrow.right").accessibilityHidden(true)
                    workflowStage("Saved checks", name: "Required verification", detail: coder?.reviewAvailable == true ? "Saved evidence ready" : workflow.checking ? "Saved evidence ready" : "Awaiting verified completion", icon: "checkmark.shield")
                    Image(systemName: "arrow.right").accessibilityHidden(true)
                    workflowStage("Reviewer", name: workflow.checkerName, detail: workflow.state == "passed" ? "Passed" : reviewer.map(statusLabel) ?? (workflow.state == "active" ? "Waiting for coder" : "Not started"), icon: "person.badge.shield.checkmark")
                }
                Text("Correction rounds used: \(workflow.corrections)").font(.caption)
                if let coder { Text("Runs on \(coder.target)").font(.caption).foregroundStyle(.secondary) }
                if let reason = workflow.reason { Text(reason).font(.caption).textSelection(.enabled) }
                if workflow.state == "active" {
                    Text("Working on: \(jobs.first { $0.id == workflow.current }?.title ?? workflow.current)").font(.caption)
                    Button("Stop workflow", role: .destructive) { Task {
                        busy = true; defer { busy = false }
                        do { try await store.stopReviewWorkflow(id: workflow.id); await refresh() }
                        catch { self.error = readableError(error) }
                    } }.disabled(busy)
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    private func workflowStage(_ title: String, name: String, detail: String, icon: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(title, systemImage: icon).font(.subheadline).bold()
            Text(name).font(.caption)
            Text(detail).font(.caption).foregroundStyle(.secondary)
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private func startWorkflow(_ source: RemoteCodingTask) async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        let key = "\(source.id)/\(checkerTarget)/\(checkerAgent)/\(checkerModel)/\(checkerReasoningOff)/\(correctionLimit)"
        let request = workflowRequests[key] ?? UUID().uuidString
        workflowRequests[key] = request
        do {
            try await store.startReviewWorkflow(request: request, project: project.id, source: source.id, target: checkerTarget, agent: checkerAgent, model: checkerModel, codingThink: checkerReasoningOff ? false : nil, corrections: UInt32(correctionLimit))
            reviewSource = nil
            message = "Workflow started. Follow its status below; stop it here at any time."
            await refresh()
        } catch { self.error = readableError(error) }
    }
    private func stageCorrection(_ review: RemoteCodingTask) async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        let request = correctionRequests[review.id] ?? UUID().uuidString
        correctionRequests[review.id] = request
        do {
            try await store.stageCorrection(request: request, project: project.id, review: review.id)
            message = "Correction task saved. Prepare and run it below, then request a new independent check."
            await refresh()
        } catch { self.error = readableError(error) }
    }
    private func statusLabel(_ job: RemoteCodingTask) -> String {
        if let verdict = job.checkerVerdict {
            switch verdict {
            case "pass": return "Snapshot check passed"
            case "changes_required": return "Changes needed"
            default: return "Check inconclusive"
            }
        }
        if job.status == "review" { return "Ready for review" }
        if let state = job.runState { return state.capitalized }
        if job.preparationState == "prepared" { return "Ready to run" }
        if job.preparationState == "claimed" { return "Preparing on \(job.target)" }
        if job.preparationState == "queued" { return "Waiting for \(job.target)" }
        return "Needs preparation"
    }
    private func readableError(_ error: Error) -> String {
        if let hive = error as? HiveError, case .Failed(let message) = hive { return message }
        return error.localizedDescription
    }
    private func refresh() async {
        do {
            let freshHosts = try await store.codingHosts()
            let freshAgents = try await store.codingAgents()
            let freshWorkflows = try await store.reviewWorkflows(project: project.id)
            let freshJobs = try await store.remoteCodingTasks(project: project.id)
            hosts = freshHosts; agents = freshAgents; workflows = freshWorkflows; jobs = freshJobs
            error = nil
        } catch { self.error = readableError(error) }
    }
    private func stage() async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        do {
            try await store.stageRemoteCoding(request: requestID, project: project.id, target: target, title: title, task: instructions, model: model, turns: UInt32(turns), checks: checks.map(\.record), agent: agent.isEmpty ? nil : agent, codingThink: reasoningOff ? false : nil, verificationRuns: allowVerification && !checks.isEmpty ? 1 : 0)
            requestID = UUID().uuidString; title = ""; instructions = ""; checks = []; allowVerification = false
            showingNewTask = false
            await refresh()
        } catch { self.error = readableError(error) }
    }
    private func command(_ action: String, _ job: RemoteCodingTask) async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        let key = "\(job.id)/\(action)/\(job.runId ?? job.preparationId ?? "initial")"
        let request = commandIDs[key] ?? UUID().uuidString
        commandIDs[key] = request
        do {
            try await store.codingCommand(action: action, task: job.id, operation: action == "recover" ? (job.preparationId ?? "") : (job.runId ?? ""), request: request)
            if action == "recover" { commandIDs.removeValue(forKey: key) }
            message = action == "recover" ? "Recovery requested. Restart the coding worker on \(job.target)." : "Request saved for \(job.target)."
        } catch { self.error = readableError(error) }
        await refresh()
    }
}
