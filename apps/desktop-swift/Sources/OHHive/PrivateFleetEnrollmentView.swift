import SwiftUI

/// Initial explicit owner-approval flow. Joining a remote primary is a separate selection step.
struct PrivateFleetEnrollmentView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var request = ""
    @State private var fingerprint = ""
    @State private var authority = ""
    @State private var approval = ""
    @State private var busy = false
    @State private var message: String?

    var body: some View {
        GroupBox("Private Fleet identity") {
            VStack(alignment: .leading, spacing: 10) {
                if store.snapshot?.privateFleetEnrolled == true {
                    Label("This Mac has a verified Private Fleet identity", systemImage: "checkmark.circle")
                    Text("Open Bots for your conversations. Use Primary computer below to connect your other computers.")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    Text("Set up this Mac as your first private computer. You can sign in with Apple or Google without an OHG community invitation.")
                    Text("If you already have a primary computer, use Connect to a primary below instead of enrolling this Mac separately.")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("Create connection request") { Task { await begin() } }.disabled(busy)
                    if !request.isEmpty {
                        Text("Computer fingerprint: \(fingerprint)").textSelection(.enabled)
                        Text("Primary identifier: \(authority)").font(.caption).textSelection(.enabled)
                        ShareLink("Copy or share connection request", item: request)
                        Link("Sign in and approve on the Hive website", destination: URL(string: "https://ohghive.com/private-fleet/enroll")!)
                        Text("Paste the request on the website, compare these details, then return with your approval. Requests expire in five minutes.")
                            .font(.caption).foregroundStyle(.secondary)
                        TextField("Paste your approval here", text: $approval, axis: .vertical)
                            .lineLimit(3...5).textFieldStyle(.roundedBorder).disabled(busy)
                        Button("Finish Private Fleet setup") { Task { await complete() } }
                            .disabled(busy || approval.isEmpty)
                    }
                }
                if busy { ProgressView().controlSize(.small) }
                if let message { Text(message).font(.caption).textSelection(.enabled) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    @MainActor private func begin() async {
        busy = true; message = nil; approval = ""; request = ""
        defer { busy = false }
        do {
            let value = try await store.privateFleetEnrollmentBegin()
            let details = try JSONDecoder().decode(ConnectionDetails.self, from: Data(value.utf8))
            request = value; fingerprint = String(details.credential_sha256.prefix(12)); authority = details.authority_id
        } catch { message = error.localizedDescription }
    }
    @MainActor private func complete() async {
        busy = true; message = nil
        defer { busy = false }
        do {
            try await store.privateFleetEnrollmentComplete(approval: approval)
            approval = ""; request = ""; message = "Identity verified. Your Private Fleet is ready for local Bots."
        } catch { message = error.localizedDescription }
    }
    private struct ConnectionDetails: Decodable {
        let credential_sha256: String
        let authority_id: String
    }
}
