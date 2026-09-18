import SwiftUI
import OHHiveFFI
import UniformTypeIdentifiers

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
    @State private var choosesImage = false
    @State private var importingImage = false

    /// The realm this agent runs on, as the vault records it. Read-only on purpose: the name
    /// follows the computer, so renaming the computer renames the realm everywhere at once
    /// rather than leaving each agent carrying its own stale copy.
    private var realm: String {
        if let host = agent.hostName, !host.isEmpty {
            return agent.preferredHost == model.hostID ? "\(host) — this Mac" : host
        }
        if agent.preferredHost == nil { return "No computer — answers through the Hive" }
        return "A computer this vault doesn't know"
    }
    private var changed: Bool { name != agent.name || profile != original }
    private var valid: Bool { !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && name.utf8.count <= 200 && profile.isValid }
    var body: some View {
        Form {
            Section("Agent profile") {
                HStack { Spacer(); AgentAvatar(name: profile.avatar); Spacer() }
                TextField("Name", text: $name)
                LabeledContent("Realm", value: realm)
                Text("The computer this agent lives on. It follows the computer's name — rename the computer to rename the realm.")
                    .font(.caption).foregroundStyle(.secondary)
                Picker("Avatar", selection: $profile.avatar) {
                    Text("Default").tag("")
                    if profile.avatar.hasPrefix(AvatarUpload.prefix) { Text("Uploaded image").tag(profile.avatar) }
                    ForEach(AgentAvatar.choices, id: \.self) { avatar in Text(avatar.capitalized).tag(avatar) }
                }
                Button(importingImage ? "Preparing image…" : "Upload your own…", systemImage: "photo.badge.plus") { choosesImage = true }
                    .disabled(!loaded || importingImage)
                Text("Recommended: a 512 × 512 square image. Keep the face or logo centered; avatars appear in a circle. PNG, JPEG, HEIC or GIF, up to 10 MB. We resize and center-crop it for you; GIFs use the first frame.")
                    .font(.caption).foregroundStyle(.secondary)
                Text("Choose an image, then Save profile to share it with your fleet.").font(.caption).foregroundStyle(.secondary)
            }
            AgentToolsSection(agentID: agent.id, isLocal: agent.runtimeKind == "local", model: model, biography: $profile)
                .id(agent.id)
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
                LabeledContent("Realm", value: realm)
                Text("Library tools are managed in Template and tools above.").font(.caption)
                Text(agent.id).font(.caption.monospaced()).textSelection(.enabled)
            }
            Section {
                Button("Delete agent…", role: .destructive) { confirmsDelete = true }
                    .disabled(saving || !model.paired)
            }
        }
        .formStyle(.grouped)
        .disabled(saving || importingImage)
        .fileImporter(isPresented: $choosesImage, allowedContentTypes: [.png, .jpeg, .heic, .gif]) { result in
            guard case .success(let url) = result else {
                if case .failure(let error) = result { saveError = error.localizedDescription }; return
            }
            importingImage = true; saveError = nil
            Task {
                defer { importingImage = false }
                do {
                    let value = try await Task.detached { try AvatarUpload.read(url) }.value
                    guard !Task.isCancelled else { return }
                    profile.avatar = value; saved = false
                } catch { saveError = error.localizedDescription }
            }
        }
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
