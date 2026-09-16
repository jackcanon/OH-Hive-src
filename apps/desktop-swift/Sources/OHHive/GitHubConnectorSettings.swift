import SwiftUI

struct GitHubConnectorSettings: View {
    @EnvironmentObject private var github: GitHubAuthManager
    var body: some View {
        GroupBox("GitHub") {
            VStack(alignment: .leading, spacing: 10) {
                Text("Connect on this Mac to list up to 100 public repositories accessible to your account. This does not grant private-repository access, push code, or publish issues.")
                    .font(.caption).foregroundStyle(.secondary)
                if github.isConnected {
                    Text(github.login.map { "Connected as \($0) — this Mac only" } ?? "Connected — this Mac only")
                    HStack {
                        Button("Load Public Repositories") { Task { await github.loadPublicRepositories() } }
                        Button("Disconnect") { github.disconnect() }
                    }.disabled(github.busy)
                    ForEach(github.repositories) { repository in
                        if let url = repository.safeURL { Link(repository.full_name, destination: url) }
                    }
                } else {
                    SettingsNote("Shared GitHub sign-in is being configured for this build. No personal OAuth client setup is required.")
                    Button("Connect GitHub") { }
                        .disabled(true)
                }
                if github.busy { ProgressView().controlSize(.small) }
                if let error = github.lastError { SettingsNote(error) }
                Text("Disconnect removes this Mac’s credentials. To revoke GitHub authorization, use GitHub Settings > Applications.")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            .textFieldStyle(.roundedBorder)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
