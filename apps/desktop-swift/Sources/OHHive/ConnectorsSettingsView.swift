import SwiftUI

/// ADR-026 Decision 5: a "Connectors" settings section, one card per service, with the same
/// "here's exactly what this can and can't do" framing as Decision 4 -- not vague "Drive access"
/// copy. v1 (2026-09-14, Jack: "let's build out Google Workspace to start") is Google Drive +
/// Gmail as one connection, one consent screen, both scopes together -- ADR-026 always described
/// them as a pair, not two separate per-service toggles. Future connectors (providers 3+) get
/// their own card here once ADR-026's still-open per-machine-vs-broker question is answered.
struct ConnectorsSettingsView: View {
    @StateObject private var google = GoogleAuthManager()
    @State private var showCredentialFields = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            GroupBox("Google Workspace (Drive + Gmail)") {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Save files a chat or feedback session explicitly creates to your Drive, and send email as you. This cannot browse your existing Drive files or read your inbox \u{2014} that's a deliberate v1 limit, not a bug. See ADR-026 for why.")
                        .font(.caption).foregroundStyle(.secondary)

                    if google.isConnected {
                        HStack {
                            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                            Text("Connected \u{2014} this Mac only")
                            Spacer()
                            Button("Disconnect") { google.disconnect() }
                                .buttonStyle(.link)
                        }
                    } else {
                        DisclosureGroup("Google OAuth client (one-time setup)", isExpanded: $showCredentialFields) {
                            VStack(alignment: .leading, spacing: 8) {
                                Text("From a Google Cloud project with the Drive and Gmail APIs enabled, OAuth client type ‘Desktop app.’ Ask Loki for the exact Cloud Console steps if you haven't done this before.")
                                    .font(.caption).foregroundStyle(.secondary)
                                TextField("Client ID", text: $google.clientID)
                                    .textFieldStyle(.roundedBorder)
                                SecureField("Client secret", text: $google.clientSecret)
                                    .textFieldStyle(.roundedBorder)
                                Button("Save") {
                                    google.saveCredentials(id: google.clientID, secret: google.clientSecret)
                                }
                                .disabled(google.clientID.trimmingCharacters(in: .whitespaces).isEmpty
                                    || google.clientSecret.trimmingCharacters(in: .whitespaces).isEmpty)
                            }
                            .padding(.top, 6)
                        }

                        Button {
                            Task { await google.connect() }
                        } label: {
                            if google.isConnecting {
                                ProgressView().controlSize(.small)
                            } else {
                                Text("Connect Google")
                            }
                        }
                        .disabled(google.isConnecting || google.clientID.isEmpty || google.clientSecret.isEmpty)
                    }

                    if let err = google.lastError {
                        Text(err).font(.caption).foregroundStyle(.red)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            Text("More connectors (providers 3+) are pre-1.0 planning work — see ADR-026.")
                .font(.caption2).foregroundStyle(.secondary)
        }
    }
}
