import SwiftUI
import OHHiveFFI

/// Room metadata is private. Selecting a community project only stores its reference here.
struct BotsNewRoomView: View {
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var store: HiveStore
    let model: BotsModel
    @State private var title = ""
    @State private var members: Set<String> = []
    @State private var coordinator = ""
    @State private var project = ""
    @State private var projects: [RoomProject] = []
    @State private var projectNote: String?
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("New room").font(.title2)
            TextField("Room name", text: $title).textFieldStyle(.roundedBorder)
            Picker("Project", selection: $project) {
                Text("None — team room").tag("")
                ForEach(projects) { p in Text(p.title).tag(p.id) }
            }
            Button("Load my Hive projects") { Task { await loadProjects() } }
            if let projectNote { Text(projectNote).font(.caption).foregroundStyle(.secondary) }
            Text("Choose up to 16 agents. For this demo, replies run on this Mac.").font(.caption).foregroundStyle(.secondary)
            ScrollView {
                VStack(alignment: .leading) {
                    ForEach(model.agents, id: \.id) { agent in
                        Toggle(agent.name, isOn: Binding(get: { members.contains(agent.id) }, set: { enabled in
                            if enabled { members.insert(agent.id) } else { members.remove(agent.id); if coordinator == agent.id { coordinator = "" } }
                        }))
                        .disabled(!members.contains(agent.id) && members.count >= 16)
                    }
                }
            }.frame(maxHeight: 200)
            Picker("Coordinator", selection: $coordinator) {
                Text("None").tag("")
                ForEach(model.agents.filter { members.contains($0.id) }, id: \.id) { agent in Text(agent.name).tag(agent.id) }
            }
            Text("Use @names or @everyone to request replies. Agents do not trigger one another yet.").font(.caption).foregroundStyle(.secondary)
            if let error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Creating…" : "Create room") {
                    busy = true
                    Task {
                        defer { busy = false }
                        do {
                            try await model.createRoom(title: title, agentIDs: members.sorted(), projectID: project.isEmpty ? nil : project, coordinatorID: coordinator.isEmpty ? nil : coordinator)
                            dismiss()
                        } catch { self.error = error.localizedDescription }
                    }
                }.buttonStyle(.borderedProminent).disabled(members.isEmpty || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || title.utf8.count > 200)
            }
        }.padding(24).frame(width: 440).disabled(busy)
    }

    private func loadProjects() async {
        guard let wire = await store.kanbanCloudProjects(), let data = wire.data(using: .utf8) else {
            projectNote = "Projects could not be loaded. Connect to your community Hive to choose one."; return
        }
        do {
            projects = try JSONDecoder().decode([RoomProject].self, from: data)
            projectNote = projects.isEmpty ? "No projects available." : "Chat stays private; this links the room to a project."
        } catch { projectNote = "Could not read the project list." }
    }
}
private struct RoomProject: Decodable, Identifiable { let id: String; let title: String }
