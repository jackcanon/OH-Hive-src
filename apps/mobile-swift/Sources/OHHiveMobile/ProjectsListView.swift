import SwiftUI

struct ProjectsListView: View {
    @State private var projects: [CloudProject] = []
    @State private var error: String?
    @State private var showingNew = false

    var body: some View {
        List {
            if let error { Text(error).font(.caption).foregroundStyle(.secondary) }
            ForEach(projects) { project in
                NavigationLink(value: project.id) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(project.title).font(.body.weight(.medium))
                        Text(project.goal).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    }
                }
            }
        }
        .navigationTitle("Projects")
        .navigationDestination(for: String.self) { id in ProjectDetailView(projectId: id) }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button { showingNew = true } label: { Image(systemName: "plus") }
            }
        }
        .sheet(isPresented: $showingNew) {
            NavigationStack { NewProjectView() }
        }
        .task { await load() }
        .refreshable { await load() }
    }

    private func load() async {
        do {
            projects = try await supabase.rpc("hive_projects_overview").execute().value
            error = nil
        } catch {
            self.error = "Couldn't load projects (\(error.localizedDescription))."
        }
    }
}
