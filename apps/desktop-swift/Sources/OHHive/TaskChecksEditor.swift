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
    @Binding var checks: [TaskCheckDraft]
    var body: some View {
        DisclosureGroup("Acceptance checks (\(checks.count))") {
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Example: program npm, argument test. Enter each argument on its own line, without surrounding quotes. Programs run from the repository root; success means exit code 0.")
                        .font(.caption).foregroundStyle(.secondary)
                    ForEach($checks) { $check in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                TextField("Check name", text: $check.name)
                                Button("Remove") { checks.removeAll { $0.id == check.id } }
                                    .accessibilityLabel("Remove \(check.name.isEmpty ? "check" : check.name)")
                            }
                            TextField("Program (for example npm)", text: $check.command)
                            TextField("Arguments — one per line", text: $check.arguments, axis: .vertical)
                                .lineLimit(2...4)
                        }
                    }
                    Button("Add check") { checks.append(TaskCheckDraft()) }.disabled(checks.count >= 16)
                }
            }.frame(maxHeight: 190)
        }
    }
}
