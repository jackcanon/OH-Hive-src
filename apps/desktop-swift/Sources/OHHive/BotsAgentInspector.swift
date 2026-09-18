import SwiftUI
import OHHiveFFI

struct BotsAgentInspector: View {
    let agent: BotsAgent
    let model: BotsModel
    @State private var name = ""
    @State private var profile = AgentBiography()
    @State private var original = AgentBiography()
    @State private var loaded = false
    @State private var saving = false
    @State private var saveError: String?
    @State private var saved = false
    @State private var confirmsDelete = false

    private var changed: Bool { name != agent.name || profile != original }
    private var valid: Bool { !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.utf8.count <= 200 && profile.isValid }
    var body: some View {
        Form {
            Section("Agent profile") {
                HStack { Spacer(); AgentAvatar(name: profile.avatar); Spacer() }
                TextField("Name", text: $name)
                Picker("Avatar", selection: $profile.avatar) {
                    Text("Default").tag("")
                    ForEach(AgentAvatar.choices, id: \.self) { avatar in Text(avatar.capitalized).tag(avatar) }
                }
            }
            Section("Bio") {
                Text("Who this agent is and what they help with.").font(.caption).foregroundStyle(.secondary)
                TextEditor(text: $profile.bio).frame(minHeight: 80).accessibilityLabel("Agent biography")
            }
            Section("Instructions") {
                Text("How this agent should work and respond. Saved changes apply to future replies; an answer already running may use earlier instructions.").font(.caption).foregroundStyle(.secondary)
                TextEditor(text: $profile.instructions).frame(minHeight: 150).accessibilityLabel("Agent instructions")
                Text("Instructions do not enable tools or change the selected model.").font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Button(saving ? "Saving…" : "Save profile") { save() }
                    .disabled(!loaded || saving || !changed || !valid || !model.paired)
                Button("Reload saved profile") { Task { await load() } }.disabled(saving)
                if let saveError { Text(saveError).foregroundStyle(.red).textSelection(.enabled) }
                if saved && !changed { Label("Saved", systemImage: "checkmark.circle") }
                if !valid { Text("Keep the name under 200 bytes, bio under 4,000 and instructions under 16,000.").font(.caption) }
            }
            DisclosureGroup("Connection details") {
                LabeledContent("Runtime", value: agent.runtimeKind == "local" ? "Local model" : agent.runtimeKind)
                LabeledContent("Host", value: agent.preferredHost == model.hostID ? "This Mac" : "Fleet agent")
                Text("Bots replies currently have no tool access.").font(.caption)
                Text(agent.id).font(.caption.monospaced()).textSelection(.enabled)
            }
            Section {
                Button("Delete agent…", role: .destructive) { confirmsDelete = true }
                    .disabled(saving || !model.paired)
            }
        }
        .formStyle(.grouped)
        .disabled(saving)
        .task(id: agent.id) { await load() }
        .confirmationDialog("Delete \(agent.name)?", isPresented: $confirmsDelete, titleVisibility: .visible) {
            Button("Delete agent", role: .destructive) {
                saving = true
                Task {
                    defer { saving = false }
                    do { try await model.deleteAgent(agent.id) }
                    catch { saveError = String(describing: error) }
                }
            }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("Removes this agent from your fleet roster and stops new replies. Conversation history is kept. A reply already running may finish. This does not delete models or disconnect the computer.")
        }
    }
    private func load() async {
        loaded = false; saveError = nil
        do {
            let result = try await model.agentBio(agent.id)
            guard !Task.isCancelled else { return }
            profile = result; original = result; name = model.agents.first(where: { $0.id == agent.id })?.name ?? agent.name; loaded = true
        } catch { saveError = String(describing: error) }
    }
    private func save() {
        guard loaded, valid, !saving else { return }
        saving = true; saveError = nil; saved = false
        let snapshot = profile; let nextName = name
        Task {
            defer { saving = false }
            do {
                try await model.saveAgentBio(agent.id, name: nextName, profile: snapshot)
                await load(); saved = loaded
            } catch { saveError = String(describing: error) }
        }
    }
}
