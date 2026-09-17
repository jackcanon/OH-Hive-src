import SwiftUI
import OHHiveFFI

/// Real execution projects, separate from the local-only idea board beneath this section.
struct RepositoryProjectsView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var projects: [PrivateRepositoryProject] = []
    @State private var title = ""
    @State private var goal = ""
    @State private var busy = false
    @State private var ready = false
    @State private var showingSetup = false
    @State private var error: String?
    @State private var editing: PrivateRepositoryProject?
    private struct TaskSelection: Identifiable {
        let project: PrivateRepositoryProject
        var id: String { project.id }
    }
    @State private var tasksProject: TaskSelection?

    var body: some View {
        GroupBox("Coding projects") {
            VStack(alignment: .leading, spacing: 12) {
                Text("Connect a repository to a project on this primary computer. New coding tasks can use it; existing tasks keep their original repository.")
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    TextField("Project name", text: $title)
                    TextField("Project goal (optional)", text: $goal)
                    Button("Create project") { Task { await create() } }
                        .disabled(!ready || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("Refresh") { Task { await refresh() } }
                }.disabled(busy)
                if busy { ProgressView().controlSize(.small) }
                if let error {
                    Text(error).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    Button("Open Private Fleet setup") { showingSetup = true }
                        .disabled(busy)
                }
                ForEach(projects, id: \.id) { project in
                    HStack {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(project.title).font(.headline)
                            Text(project.repoUrl ?? "No repository connected")
                                .font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                            if let reference = project.repoRef { Text("Reference: \(reference)").font(.caption) }
                        }
                        Spacer()
                        Button("Tasks…") { tasksProject = TaskSelection(project: project) }.disabled(busy || !ready)
                        Button("Repository…") { editing = project }.disabled(busy || !ready)
                    }
                }
                if ready && projects.isEmpty { Text("Create your first coding project above.").font(.caption) }
            }
            .textFieldStyle(.roundedBorder)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .task { await refresh() }
        .sheet(isPresented: $showingSetup, onDismiss: { Task { await refresh() } }) {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Register this Mac").font(.title2)
                    Spacer()
                    Button("Done") { showingSetup = false }
                }
                ScrollView {
                    PrivateFleetEnrollmentView()
                }
            }.padding(24).frame(width: 580, height: 420)
        }
        .sheet(item: $tasksProject) { selection in
            PrivateCodingTasksView(project: selection.project)
        }

        .sheet(isPresented: Binding(get: { editing != nil }, set: { if !$0 { editing = nil } })) {
            if let project = editing {
                ProjectRepositoryEditor(project: project) {
                    editing = nil
                    await refresh()
                }
            }
        }
    }

    private func refresh() async {
        busy = true
        defer { busy = false }
        do {
            projects = try await store.repositoryProjects()
            ready = true
            error = nil
        } catch {
            projects = []
            ready = false
            if let hiveError = error as? HiveError, case .Failed(let message) = hiveError {
                self.error = message
            } else { self.error = error.localizedDescription }
        }
    }

    private func create() async {
        busy = true
        do {
            try await store.createRepositoryProject(title: title.trimmingCharacters(in: .whitespacesAndNewlines), goal: goal)
            title = ""
            goal = ""
            await refresh()
        } catch { self.error = String(describing: error) }
        busy = false
    }
}

private struct ProjectRepositoryEditor: View {
    @EnvironmentObject private var store: HiveStore
    @EnvironmentObject private var github: GitHubAuthManager
    @Environment(\.dismiss) private var dismiss
    let project: PrivateRepositoryProject
    let saved: () async -> Void
    @State private var url: String
    @State private var reference: String
    @State private var busy = false
    @State private var error: String?
    @State private var accessResult: String?

    init(project: PrivateRepositoryProject, saved: @escaping () async -> Void) {
        self.project = project
        self.saved = saved
        // A deliberate editable snapshot for this sheet; cancel discards changes.
        _url = State(initialValue: project.repoUrl ?? "")
        _reference = State(initialValue: project.repoRef ?? "")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Repository for \(project.title)").font(.headline)
            if github.isConnected {
                HStack {
                    Menu("Choose a GitHub repository") {
                        ForEach(github.repositories) { repo in
                            Button(repo.full_name) { url = "https://github.com/\(repo.full_name).git" }
                        }
                    }.disabled(github.repositories.isEmpty)
                    Button("Load repositories") { Task { await github.loadRepositories() } }
                        .disabled(github.busy)
                    if github.busy { ProgressView().controlSize(.small) }
                }
                if let message = github.lastError { Text(message).font(.caption) }
            } else {
                Text("Connect GitHub in Settings → Connectors to choose from your repositories, or enter a GitHub URL below.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            TextField("https://github.com/owner/repository.git", text: $url)
            TextField("Branch, tag or commit (blank uses the default branch)", text: $reference)
            if github.isConnected, project.repoUrl != nil {
                Button("Check saved repository access") { Task { await checkAccess() } }
                    .disabled(github.busy || url != project.repoUrl)
                if let accessResult { Text(accessResult).font(.caption).foregroundStyle(.secondary) }
            }
            Text("Save the repository choice, then open Tasks to prepare a checkout and run a task on this Mac. Preparation uses your GitHub connection. Saving does not clone, push or publish code.")
                .font(.caption).foregroundStyle(.secondary)
            if let error { Text(error).font(.caption).foregroundStyle(.red) }
            HStack {
                Button("Disconnect repository") { Task { await save(clear: true) } }
                    .disabled(project.repoUrl == nil)
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Save") { Task { await save(clear: false) } }
                    .disabled(url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .textFieldStyle(.roundedBorder)
        .padding(24)
        .frame(width: 600)
        .disabled(busy)
        .interactiveDismissDisabled(busy)
    }

    private func save(clear: Bool) async {
        busy = true
        defer { busy = false }
        let ref = reference.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            try await store.setProjectRepository(id: project.id,
                url: clear ? nil : url.trimmingCharacters(in: .whitespacesAndNewlines),
                reference: clear || ref.isEmpty ? nil : ref)
            await saved()
        } catch { self.error = String(describing: error) }
    }

    private func checkAccess() async {
        busy = true
        error = nil
        accessResult = nil
        defer { busy = false }
        do {
            try await github.withRepositoryGitToken { token in
                try await store.checkProjectRepository(id: project.id, token: token)
            }
            accessResult = "Git can read the saved repository from this Mac. No files were downloaded or changed. This does not enable other workers yet."
        } catch { self.error = String(describing: error) }
    }
}
