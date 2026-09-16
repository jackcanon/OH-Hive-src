import SwiftUI

struct GitHubConnectorSettings: View {
    @EnvironmentObject private var github: GitHubAuthManager
    var body: some View {
        GroupBox("GitHub") {
            VStack(alignment: .leading, spacing: 10) {
                Text("Connect on this Mac to list up to 100 public repositories accessible to your account. This screen only lists public repositories. Review the app’s permissions on GitHub before approving sign-in.")
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
                    if !github.isConfigured {
                        SettingsNote("Shared GitHub sign-in is not configured in this build. No personal OAuth client setup is required.")
                    }
                    if let code = github.userCode {
                        Text("Enter this code on GitHub to approve sign-in:")
                        Text(code).font(.title2.monospaced()).textSelection(.enabled)
                        Link("Open GitHub", destination: URL(string: "https://github.com/login/device")!)
                    }
                    if github.busy {
                        Button("Cancel Sign-In") { github.cancelConnection() }
                    } else {
                        Button("Connect GitHub") { github.connect() }
                            .disabled(!github.isConfigured)
                    }
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
