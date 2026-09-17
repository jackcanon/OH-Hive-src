import SwiftUI
import OHHiveFFI

struct TaskCheckDraft: Identifiable {
    let id = UUID()
    var name = ""
    var command = ""
    var arguments = ""
    var isValid: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        !command.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    var record: PrivateTaskCheck {
        PrivateTaskCheck(name: name.trimmingCharacters(in: .whitespacesAndNewlines),
                         command: command.trimmingCharacters(in: .whitespacesAndNewlines),
                         args: arguments.split(separator: "\n").map(String.init))
    }
}

struct TaskChecksEditor: View {
    @EnvironmentObject private var github: GitHubAuthManager
    @Binding var checks: [TaskCheckDraft]
    let repository: String?
    let reference: String?
    let task: String
    @State private var report: ProjectCheckReport?
    @State private var inspecting = false
    @State private var error: String?

    var body: some View {
        DisclosureGroup("Checks and recommendations (\(checks.count) selected)") {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    Button(inspecting ? "Reading project…" : "Find suggested checks") { Task { await inspect() } }
                        .disabled(inspecting || repository == nil || !github.isConnected)
                    Text("Reads project configuration from GitHub. Nothing runs until you run the task.").font(.caption).foregroundStyle(.secondary)
                    if !github.isConnected { Text("Connect GitHub in Settings to find project checks.").font(.caption) }
                    if let error { Text(error).font(.caption).foregroundStyle(.red) }
                    if let report {
                        if !report.checks.isEmpty {
                            Button("Use recommended checks") { for suggestion in report.checks { add(suggestion) } }
                                .disabled(report.checks.filter { !contains($0) }.count > 16 - checks.count || report.checks.allSatisfy { contains($0) })
                            Text("Up to 16 checks per task. Remove a selected check to make room if needed.").font(.caption2).foregroundStyle(.secondary)
                        }
                        ForEach(report.checks) { suggestion in
                            VStack(alignment: .leading, spacing: 3) {
                                HStack {
                                    Text(suggestion.name).font(.headline)
                                    Spacer()
                                    Button(contains(suggestion) ? "Added" : "Add") { add(suggestion) }
                                        .disabled(contains(suggestion) || checks.count >= 16)
                                }
                                Text(suggestion.explanation).font(.caption)
                                Text("Found in \(suggestion.evidence)").font(.caption2).foregroundStyle(.secondary)
                                Text(([suggestion.command] + suggestion.args).joined(separator: " ")).font(.caption.monospaced())
                            }
                        }
                        ForEach(report.notes, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                    }
                    Text(task.lowercased().contains("bug") || task.lowercased().contains("fix")
                         ? "Needs creating: ask the agent for a regression test that fails before the fix and passes afterward. Existing tests may not cover this bug."
                         : "Needs creating: ask the agent for a test of the specific behavior you requested if existing tests do not cover it.")
                        .font(.caption)
                    Text("Your review: check the result against your request, especially appearance, usability and wording. Automated checks cannot confirm everything.").font(.caption)
                    ForEach(checks) { check in
                        HStack {
                            Text(check.name.isEmpty ? "Unnamed check" : check.name).font(.caption)
                            Spacer()
                            Button("Remove") { checks.removeAll { $0.id == check.id } }
                        }
                    }
                    DisclosureGroup("Advanced: edit commands") {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("Enter each argument on its own line, without surrounding quotes. Programs run from the repository root; success means exit code 0.").font(.caption).foregroundStyle(.secondary)
                            ForEach($checks) { $check in
                                VStack(alignment: .leading, spacing: 6) {
                                    TextField("Check name", text: $check.name)
                                    TextField("Program (for example npm)", text: $check.command)
                                    TextField("Arguments — one per line", text: $check.arguments, axis: .vertical).lineLimit(2...4)
                                }
                            }
                            Button("Add custom check") { checks.append(TaskCheckDraft()) }.disabled(checks.count >= 16)
                        }
                    }
                }
            }.frame(maxHeight: 220)
        }
    }
    private func contains(_ suggestion: ProjectCheckSuggestion) -> Bool {
        checks.contains { $0.record.command == suggestion.command && $0.record.args == suggestion.args }
    }
    private func add(_ suggestion: ProjectCheckSuggestion) {
        guard checks.count < 16, !contains(suggestion) else { return }
        checks.append(TaskCheckDraft(name: suggestion.name, command: suggestion.command, arguments: suggestion.args.joined(separator: "\n")))
    }
    private func inspect() async {
        guard let repository else { return }
        inspecting = true; error = nil; report = nil
        defer { inspecting = false }
        do { report = try await github.suggestProjectChecks(repository: repository, reference: reference) }
        catch { self.error = error.localizedDescription }
    }
}
