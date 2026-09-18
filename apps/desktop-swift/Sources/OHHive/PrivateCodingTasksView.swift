import SwiftUI
import OHHiveFFI

struct PrivateCodingTasksView: View {
    @EnvironmentObject private var store: HiveStore
    @Environment(\.dismiss) private var dismiss
    let project: PrivateRepositoryProject
    @State private var jobs: [RemoteCodingTask] = []
    @State private var hosts: [PrivateCodingHost] = []
    @State private var target = ""
    @State private var model = ""
    @State private var requestID = UUID().uuidString
    @State private var title = ""
    @State private var instructions = ""
    @State private var turns = 6
    @State private var checks: [TaskCheckDraft] = []
    @State private var busy = false
    @State private var error: String?
    @State private var message: String?
    @State private var confirmation: PendingCommand?
    @State private var commandIDs: [String: String] = [:]

    private struct PendingCommand {
        let job: RemoteCodingTask
        let action: String
    }
    private var selectedHost: PrivateCodingHost? { hosts.first { $0.nodeId == target } }
    private var models: [PrivateCodingModel] { selectedHost?.models.filter { $0.supportsTools != false } ?? [] }
    private var hostReady: Bool {
        guard let host = selectedHost else { return false }
        return host.fresh && host.workerEnabled && host.codingEnabled
    }

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
                    newTask
                    if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
                    if let message { Text(message).font(.caption).foregroundStyle(.secondary) }
                    HStack {
                        if busy { ProgressView().controlSize(.small) }
                        Spacer()
                        Button("Refresh computers and tasks") { Task { await refresh() } }
                    }
                    ForEach(jobs, id: \.id) { job in taskRow(job) }
                    if jobs.isEmpty { Text("No tasks yet.").foregroundStyle(.secondary) }
                }
            }
        }
        .textFieldStyle(.roundedBorder)
        .padding(24).frame(width: 760, height: 650)
        .confirmationDialog(confirmation?.action == "recover" ? "Recover preparation?" : "Start another attempt?", isPresented: Binding(get: { confirmation != nil }, set: { if !$0 { confirmation = nil } }), titleVisibility: .visible) {
            if let pending = confirmation {
                Button(pending.action == "recover" ? "Recover preparation" : "Keep files and run again") {
                    confirmation = nil
                    Task { await command(pending.action, pending.job) }
                }
            }
            Button("Cancel", role: .cancel) { confirmation = nil }
        } message: {
            Text(confirmation?.action == "recover"
                ? "Use this after the previous worker stopped. Existing files are kept. Restart the worker on the execution computer after recovery. This does not run a model."
                : "The agent will run again on the same computer with the existing files and checks. Earlier actions may be repeated. An active attempt cannot be retried.")
        }
        .onChange(of: target) { _, _ in model = "" }
        .task {
            while !Task.isCancelled {
                await refresh()
                do { try await Task.sleep(for: .seconds(5)) } catch { break }
            }
        }
    }
    private var newTask: some View {
        GroupBox("New task") {
            VStack(alignment: .leading, spacing: 8) {
                TextField("Task title", text: $title)
                TextField("What should the agent do?", text: $instructions, axis: .vertical).lineLimit(3...6)
                Picker("Run on", selection: $target) {
                    Text("Choose a computer").tag("")
                    ForEach(hosts, id: \.nodeId) { host in
                        Text(host.name + (host.fresh && host.workerEnabled && host.codingEnabled ? " · ready" : " · unavailable")).tag(host.nodeId)
                    }
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
                    Stepper("Maximum turns: \(turns)", value: $turns, in: 1...20)
                    TaskChecksEditor(checks: $checks, repository: project.repoUrl, reference: project.repoRef, task: instructions)
                }
                HStack {
                    Text(checks.isEmpty ? "No checks selected · results will be unverified." : "\(checks.count) required checks").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Save task") { Task { await stage() } }
                        .disabled(!hostReady || !models.contains { $0.id == model } || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || instructions.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || checks.contains { !$0.isValid })
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
                }.disabled(busy)
                Text(job.checkCount == 0 ? "No acceptance checks · unverified" : "\(job.checkCount) required checks").font(.caption).foregroundStyle(.secondary)
                if let reason = job.reason, !["awaiting_repository_preparation", "awaiting_private_run", "awaiting_retry_validation"].contains(reason) {
                    Text(reason.replacingOccurrences(of: "_", with: " ")).font(.caption).textSelection(.enabled)
                }
                if let output = job.output {
                    DisclosureGroup("Result") { Text(output).font(.callout).textSelection(.enabled) }
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    private func statusLabel(_ job: RemoteCodingTask) -> String {
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
            hosts = try await store.codingHosts()
            jobs = try await store.remoteCodingTasks(project: project.id)
        } catch { self.error = readableError(error) }
    }
    private func stage() async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        do {
            try await store.stageRemoteCoding(request: requestID, project: project.id, target: target, title: title, task: instructions, model: model, turns: UInt32(turns), checks: checks.map(\.record))
            requestID = UUID().uuidString; title = ""; instructions = ""; checks = []
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
