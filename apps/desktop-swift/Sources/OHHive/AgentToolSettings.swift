import SwiftUI

struct AgentToolPolicy: Codable, Equatable {
    var revision: UInt32 = 0
    var template: String?
    var readableVaults: [String] = []
    enum CodingKeys: String, CodingKey { case revision, template; case readableVaults = "readable_vaults" }
}
struct AgentToolSettings: Decodable {
    var policy: AgentToolPolicy
    var libraries: [AgentLibraryChoice]
}
struct AgentLibraryChoice: Decodable, Identifiable {
    let id: String
    let name: String
    let state: String
    let hostAccess: Bool
    enum CodingKeys: String, CodingKey { case id, name, state; case hostAccess = "host_access" }
}

/// Access is saved independently from editable bio text; template drafts never grant tools.
struct AgentToolsSection: View {
    let agentID: String
    let isLocal: Bool
    let model: BotsModel
    @Binding var biography: AgentBiography
    @State private var policy = AgentToolPolicy()
    @State private var original = AgentToolPolicy()
    @State private var libraries: [AgentLibraryChoice] = []
    @State private var template = "assistant-v1"
    @State private var loaded = false
    @State private var busy = false
    @State private var notice: String?

    var body: some View {
        Section("Template and tools") {
            if isLocal {
                Picker("Start from", selection: $template) {
                    ForEach(AgentRoleTemplate.all) { role in
                        Text(role.name).tag(role.id)
                    }
                }
                Button("Use template") {
                    guard let role = AgentRoleTemplate.find(template) else { return }
                    policy.template = role.id
                    // New role drafts start without grants; the user selects its libraries below.
                    policy.readableVaults = []
                    if biography.avatar.isEmpty { biography.avatar = role.avatar }
                    biography.bio = role.bio
                    biography.instructions = role.instructions
                    notice = "Template added to your draft. Save profile for bio and instructions; save tool access for libraries."
                }.disabled(!loaded || busy)
                if let role = AgentRoleTemplate.find(template) {
                    Text(role.bio).font(.caption)
                    Text(role.limitation).font(.caption).foregroundStyle(.secondary)
                }
                Text("All roles can read libraries you select below. A role does not change the model or enable other tools.").font(.caption).foregroundStyle(.secondary)
                if loaded && libraries.isEmpty {
                    Text("No shared libraries yet. Add a collection in Library and share it with this agent’s computer.").font(.caption)
                }
                ForEach(libraries) { library in
                    Toggle(isOn: Binding(get: { policy.readableVaults.contains(library.id) }, set: { selected in
                        policy.readableVaults.removeAll { $0 == library.id }
                        if selected { policy.readableVaults.append(library.id); policy.readableVaults.sort() }
                    })) {
                        VStack(alignment: .leading) {
                            Text(library.name)
                            if !library.hostAccess { Text("Share with the agent’s computer in Library first.").font(.caption).foregroundStyle(.secondary) }
                            else if library.state != "ready" { Text("Library is currently unavailable.").font(.caption).foregroundStyle(.secondary) }
                        }
                    }.disabled(!loaded || busy || (!library.hostAccess && !policy.readableVaults.contains(library.id)))
                }
                if policy.readableVaults.contains(where: { id in !libraries.contains(where: { $0.id == id }) }) {
                    Button("Remove unavailable library selections") { policy.readableVaults.removeAll { id in !libraries.contains { $0.id == id } } }
                }
                Button(busy ? "Saving…" : "Save tool access") {
                    busy = true; notice = nil
                    let draft = policy
                    Task {
                        defer { busy = false }
                        do { let saved = try await model.saveAgentToolPolicy(agentID, policy: draft); guard !Task.isCancelled else { return }; policy = saved; original = saved; notice = "Tool access saved." }
                        catch { notice = String(describing: error) }
                    }
                }.disabled(!loaded || busy || policy == original || policy.readableVaults.count > 32)
                Text("Up to 32 libraries. Access is checked on every call; no shell or write tools are enabled.").font(.caption).foregroundStyle(.secondary)
                Button("Reload tool access") { Task { await load() } }.disabled(busy)
            } else {
                Text("Tool templates currently require an agent running a local model. Subscription and API agents are not connected to library tools yet.").font(.caption)
            }
            if let notice { Text(notice).font(.caption).textSelection(.enabled) }
        }
        .task(id: agentID) { await load() }
    }
    private func load() async {
        loaded = false; notice = nil
        guard isLocal else { return }
        do {
            let settings = try await model.agentToolSettings(agentID)
            guard !Task.isCancelled else { return }
            policy = settings.policy; original = policy; libraries = settings.libraries
            template = policy.template.flatMap { AgentRoleTemplate.find($0)?.id } ?? "assistant-v1"
            loaded = true
        } catch { notice = "Cannot load tool access. \(error)" }
    }
}
