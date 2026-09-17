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
    @State private var turns = 6
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
                    TextField("Local model ID (optional)", text: $model)
                    HStack {
                        Stepper("Maximum turns: \(turns)", value: $turns, in: 1...20)
                        Spacer()
                        Button("Save task") { Task { await stage() } }
                            .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || instructions.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    Text("This first version has no automated acceptance checks. Review the result before using it.").font(.caption).foregroundStyle(.secondary)
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
                                    if job.status == "ready" {
                                        Button("Run on this Mac") { Task { await run(job) } }.disabled(busy)
                                    }
                                }
                                if let reason = job.reason, reason != "awaiting_repository_preparation" { Text(reason).font(.caption).textSelection(.enabled) }
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
        .task {
            while !Task.isCancelled {
                await refresh()
                do { try await Task.sleep(for: .seconds(2)) } catch { break }
            }
        }
    }

    private func statusLabel(_ job: PrivateJobStatus) -> String {
        if job.reason == "awaiting_repository_preparation" { return "Needs preparation" }
        if job.status == "review" { return "Ready for review" }
        return job.status.replacingOccurrences(of: "_", with: " ").capitalized
    }
    private func refresh() async {
        do { jobs = try await store.privateJobs(project: project.id) }
        catch { self.error = String(describing: error) }
    }
    private func stage() async {
        busy = true; error = nil; message = nil
        defer { busy = false }
        let selectedModel = model.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            try await store.stagePrivateJob(request: requestID, project: project.id,
                title: title, task: instructions, model: selectedModel.isEmpty ? nil : selectedModel, turns: UInt32(turns))
            requestID = UUID().uuidString; title = ""; instructions = ""
            await refresh()
        } catch { self.error = String(describing: error); await refresh() }
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
        } catch { self.error = String(describing: error) }
        await refresh()
    }
    private func run(_ job: PrivateJobStatus) async {
        busy = true; running = true; error = nil; message = nil
        defer { busy = false; running = false }
        do { message = try await store.runPrivateJob(project: project.id, id: job.id) }
        catch { self.error = String(describing: error) }
        await refresh()
    }
}
