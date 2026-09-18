import SwiftUI
import OHHiveFFI

struct PrivateCodingTasksView: View {
    @EnvironmentObject private var store: HiveStore
    @EnvironmentObject private var github: GitHubAuthManager
    @Environment(\.dismiss) private var dismiss
    let project: PrivateRepositoryProject
    @State private var jobs: [PrivateJobStatus] = []
    @State private var requestID = UUID().uuidString
    @State private var title = ""
    @State private var instructions = ""
    @State private var model = ""
    @State private var models: [PrivateCodingModel] = []
    @State private var loadingModels = false
    @State private var modelError: String?
    @State private var turns = 6
    @State private var retryID: String?
    @State private var checks: [TaskCheckDraft] = []
    @State private var busy = false
    @State private var running = false
    @State private var error: String?
    @State private var message: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Tasks · \(project.title)").font(.title2)
                Spacer()
                Button("Close") { dismiss() }.disabled(busy)
            }
            Text("Tasks run on this primary Mac using your local model. Prepare downloads the saved repository; Run lets the agent edit files and execute commands. Nothing is pushed to GitHub automatically.")
                .font(.caption).foregroundStyle(.secondary)
            GroupBox("New task") {
                VStack(alignment: .leading, spacing: 8) {
                    TextField("Task title", text: $title)
                    TextField("What should the agent do?", text: $instructions, axis: .vertical).lineLimit(3...6)
                    Picker("Coding model on this Mac", selection: $model) {
                        Text("Choose a model").tag("")
                        ForEach(models.filter { $0.supportsTools != false }, id: \.id) { choice in
                            Text(choice.id + (choice.supportsTools == nil ? " (tool support unconfirmed)" : "")).tag(choice.id)
                        }
                    }.disabled(loadingModels)
                    HStack {
                        Button("Refresh models") { Task { await loadModels() } }.disabled(loadingModels)
                        if loadingModels { ProgressView().controlSize(.small) }
                        Text("Models without coding tools are excluded.").font(.caption).foregroundStyle(.secondary)
                    }
                    if let modelError { Text(modelError).font(.caption).foregroundStyle(.red) }
                    HStack {
                        Stepper("Maximum turns: \(turns)", value: $turns, in: 1...20)
                        Spacer()
                        Button("Save task") { Task { await stage() } }
                            .disabled(model.isEmpty || loadingModels || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || instructions.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || checks.contains { !$0.isValid })
                    }
                    TaskChecksEditor(checks: $checks, repository: project.repoUrl, reference: project.repoRef, task: instructions)
                    Text(checks.isEmpty ? "No checks: results will be unverified." : "Checks run after the agent finishes, in this task’s checkout. Every check must exit successfully before review. These programs run with your account’s permissions.")
                        .font(.caption).foregroundStyle(.secondary)
                }.disabled(busy)
            }
            if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            if let message { Text(message).font(.caption).foregroundStyle(.secondary) }
            HStack {
                if busy { ProgressView().controlSize(.small) }
                if running { Button("Stop task") { Task { await store.stopPrivateJob() } } }
                Spacer()
                Button("Refresh") { Task { await refresh() } }
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(jobs, id: \.id) { job in
                        GroupBox {
                            VStack(alignment: .leading, spacing: 6) {
                                HStack {
                                    Text(job.title).font(.headline)
                                    Spacer()
                                    Text(statusLabel(job)).font(.caption)
                                    if job.status == "blocked", job.reason == "awaiting_repository_preparation" {
                                        Button("Prepare") { Task { await prepare(job) } }.disabled(busy)
                                    }
                                    if job.workspace != nil && (job.status == "blocked" || job.status == "running") {
                                        Button("Prepare retry…") { retryID = job.id }.disabled(busy)
                                    }
                                    if job.status == "ready" {
                                        Button("Run on this Mac") { Task { await run(job) } }.disabled(busy)
                                    }
                                }
                                if let reason = job.reason, reason != "awaiting_repository_preparation" { Text(reason).font(.caption).textSelection(.enabled) }
                                Text(job.checkCount == 0 ? "No acceptance checks · unverified" : "\(job.checkCount) required acceptance check(s)")
                                    .font(.caption).foregroundStyle(.secondary)
                                if let workspace = job.workspace { Text(workspace).font(.caption2).foregroundStyle(.secondary).textSelection(.enabled) }
                                if let output = job.output {
                                    DisclosureGroup("Result (preview)") { Text(output).font(.callout).textSelection(.enabled) }
                                }
                            }.frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    if jobs.isEmpty { Text("No tasks yet.").foregroundStyle(.secondary) }
                }
            }
        }
        .textFieldStyle(.roundedBorder)
        .padding(24).frame(width: 760, height: 650)
        .interactiveDismissDisabled(busy)
        .confirmationDialog("Prepare a fresh attempt?", isPresented: Binding(get: { retryID != nil }, set: { if !$0 { retryID = nil } }), titleVisibility: .visible) {
            if let id = retryID { Button("Keep files and prepare retry") { Task { await retry(id) } } }
            Button("Cancel", role: .cancel) { retryID = nil }
        } message: {
            Text("Existing files and checks will be kept. The agent starts a fresh attempt and may repeat earlier actions. Inspect the checkout first. An active task cannot be retried. This does not run the task; choose Run afterward.")
        }
        .task { await loadModels() }
        .task {
            while !Task.isCancelled {
                await refresh()
                do { try await Task.sleep(for: .seconds(2)) } catch { break }
            }
        }
    }

    private func readableError(_ error: Error) -> String {
        if let hiveError = error as? HiveError, case .Failed(let message) = hiveError { return message }
        return error.localizedDescription
    }

    private func loadModels() async {
        guard !loadingModels else { return }
        loadingModels = true; modelError = nil
        defer { loadingModels = false }
        do {
            models = try await store.privateCodingModels()
            if !models.contains(where: { $0.id == model && $0.supportsTools != false }) { model = "" }
            if models.allSatisfy({ $0.supportsTools == false }) {
                modelError = "No installed model reports coding-tool support. Install a compatible model on the execution computer."
            }
        } catch {
            models = []; model = ""
            modelError = "Could not load models. Check the model server and refresh."
        }
    }

    private func statusLabel(_ job: PrivateJobStatus) -> String {
        if job.reason == "awaiting_repository_preparation" { return "Needs preparation" }
        if job.status == "review" { return "Ready for review" }
        return job.status.replacingOccurrences(of: "_", with: " ").capitalized
    }
    private func refresh() async {
        do { jobs = try await store.privateJobs(project: project.id) }
        catch { self.error = readableError(error) }
    }
    private func stage() async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        let selectedModel = model.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            try await store.stagePrivateJob(request: requestID, project: project.id,
                title: title, task: instructions, model: selectedModel.isEmpty ? nil : selectedModel, turns: UInt32(turns), checks: checks.map(\.record))
            requestID = UUID().uuidString; title = ""; instructions = ""; checks = []
            await refresh()
        } catch { self.error = readableError(error); await refresh() }
    }
    private func prepare(_ job: PrivateJobStatus) async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        do {
            if github.isConnected {
                try await github.withRepositoryGitToken { token in try await store.preparePrivateJob(id: job.id, token: token) }
            } else {
                // A fully prepared checkout can be recovered offline without a token.
                try await store.preparePrivateJob(id: job.id, token: "")
            }
            message = "Workspace prepared. Run when you’re ready."
        } catch { self.error = readableError(error) }
        await refresh()
    }
    private func retry(_ id: String) async {
        retryID = nil; busy = true; error = nil; message = nil
        defer { busy = false }
        do {
            try await store.retryPrivateJob(id: id)
            message = "Existing checkout verified. Task is ready for a fresh run."
        } catch { self.error = readableError(error) }
        await refresh()
    }
    private func run(_ job: PrivateJobStatus) async {
        busy = true; running = true; error = nil; message = nil
        defer { busy = false; running = false }
        do { message = try await store.runPrivateJob(project: project.id, id: job.id) }
        catch { self.error = readableError(error) }
        await refresh()
    }
}
