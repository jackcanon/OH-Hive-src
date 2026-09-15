import SwiftUI
import OHHiveFFI

/// Metadata editing only: the current Bots runner has no tools or policy enforcement.
struct BotsAgentInspector: View {
    let agent: BotsAgent
    let model: BotsModel
    @State private var name = ""
    @State private var policy = ""
    @State private var saving = false
    @State private var saveError: String?
    @State private var saved = false

    private var runtime: String {
        switch agent.runtimeKind {
        case "local": "Local model"
        case "anthropic_byok": "Claude API key"
        case "nous_byok": "Nous API key"
        case "chatgpt_subscription": "ChatGPT subscription"
        case "copilot_subscription": "GitHub Copilot subscription"
        case "grok_subscription": "Grok subscription"
        default: agent.runtimeKind
        }
    }
    private var isHere: Bool { agent.preferredHost == model.hostID }
    private var changed: Bool { name != agent.name || policy != agent.capabilityPolicyRef }
    private var valid: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.utf8.count <= 200 &&
        !policy.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && policy.utf8.count <= 500
    }

    var body: some View {
        Form {
            Section("Agent details") {
                TextField("Name", text: $name).onSubmit { save() }
                LabeledContent("Runtime", value: runtime)
                LabeledContent("Host", value: agent.preferredHost == nil ? "Unassigned" : (isHere ? "This Mac" : "Another computer"))
                if isHere && agent.runtimeKind == "local" {
                    Text(model.workerStatus).font(.callout).foregroundStyle(.secondary)
                } else {
                    Text("Live status is unavailable for this agent.").font(.callout).foregroundStyle(.secondary)
                }
            }
            Section("Tools") {
                Text("This agent’s Bots replies currently have no tool access.").font(.callout)
                TextField("Capability policy reference", text: $policy).onSubmit { save() }
                    .accessibilityHint("Raw reference only. Tool permissions are not enforced by this field yet.")
                Text("Raw reference only — not yet enforced. Editing this field does not enable or restrict tools.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Button(saving ? "Saving…" : "Save changes") { save() }
                    .disabled(saving || !changed || !valid || !model.paired)
                if let saveError { Text(saveError).foregroundStyle(.red).textSelection(.enabled) }
                if saved && !changed { Label("Saved", systemImage: "checkmark.circle").foregroundStyle(.secondary) }
                if !valid { Text("Use a name up to 200 bytes and a nonempty policy reference up to 500 bytes.").font(.caption).foregroundStyle(.secondary) }
            }
            Section("Identity") {
                LabeledContent("Role revision", value: String(agent.roleRevision))
                Text(agent.id).font(.caption.monospaced()).textSelection(.enabled)
                    .accessibilityLabel("Agent ID: \(agent.id)")
            }
        }
        .formStyle(.grouped)
        .task(id: agent.id) { name = agent.name; policy = agent.capabilityPolicyRef }
    }

    private func save() {
        guard valid, changed, !saving, model.paired else { return }
        saving = true; saveError = nil; saved = false
        let nextName = name == agent.name ? nil : name
        let nextPolicy = policy == agent.capabilityPolicyRef ? nil : policy
        Task {
            defer { saving = false }
            do {
                try await model.update(agentID: agent.id, name: nextName, capabilityPolicyRef: nextPolicy)
                saved = true
            } catch is CancellationError { }
            catch {
                if let hive = error as? HiveError, case .Failed(let message) = hive { saveError = message }
                else { saveError = error.localizedDescription }
            }
        }
    }
}
