import SwiftUI

struct GitHubConnectorSettings: View {
    @EnvironmentObject private var github: GitHubAuthManager
    @State private var repositoriesExpanded = false

    var body: some View {
        GroupBox("GitHub") {
            VStack(alignment: .leading, spacing: 10) {
                Text("Browse repositories shared with Loki’s Den, including private repositories, plus up to 100 public repositories from your account. Private access requires installing the GitHub App on the selected repositories.")
                    .font(.caption).foregroundStyle(.secondary)
                if github.isConnected {
                    Text(github.login.map { "Connected as \($0) — this Mac only" } ?? "Connected — this Mac only")
                    HStack {
                        Button("Load Repositories") { Task { await github.loadRepositories() } }
                        Button("Disconnect") { github.disconnect() }
                    }.disabled(github.busy)
                    Link("Choose repositories on GitHub", destination: URL(string: "https://github.com/apps/loki-s-den/installations/new")!)
                    Text("Choose which repositories to share on GitHub, then reload this list. This does not invite anyone to a Hive or publish your code.").font(.caption)
                    if !github.repositories.isEmpty {
                        DisclosureGroup("Repositories (\(github.repositories.count))", isExpanded: $repositoriesExpanded) {
                            ScrollView {
                                LazyVStack(alignment: .leading, spacing: 8) {
                                    ForEach(github.repositories) { repository in
                                        if let url = repository.safeURL {
                                            Link(repository.full_name + (repository.private == true ? " · Private" : ""), destination: url)
                                        }
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.vertical, 6)
                            }
                            .frame(height: min(CGFloat(github.repositories.count) * 30 + 12, 240))
                        }
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
