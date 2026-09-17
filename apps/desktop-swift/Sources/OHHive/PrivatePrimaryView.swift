import SwiftUI
import OHHiveFFI

/// Endpoint selection only. Moving a primary's data and execution leases is a later handover flow.
struct PrivatePrimaryView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var status: PrivatePrimaryStatus?
    @State private var action = "host"
    @State private var address = ""
    @State private var endpoint = ""
    @State private var code = ""
    @State private var pairingCode = ""
    @State private var request = ""
    @State private var fingerprint = ""
    @State private var authority = ""
    @State private var approval = ""
    @State private var busy = false
    @State private var error: String?

    init(initialAction: String = "host") {
        _action = State(initialValue: initialAction)
    }

    var body: some View {
        GroupBox("Primary computer") {
            VStack(alignment: .leading, spacing: 10) {
                if let status {
                    Label(status.mode == "secondary" ? "This Mac is a secondary" : "This Mac uses its local history", systemImage: "desktopcomputer")
                    if let endpoint = status.endpoint { Text(endpoint).font(.caption).textSelection(.enabled) }
                    Text(status.detail).font(.caption).foregroundStyle(.secondary)
                }
                Picker("Connect your fleet", selection: $action) {
                    Text("Share from this Mac").tag("host")
                    Text("Connect to a primary").tag("join")
                }.disabled(busy)
                if action == "host" {
                    TextField("This Mac’s LAN address, e.g. 192.168.1.10:8787", text: $address).textFieldStyle(.roundedBorder)
                    HStack {
                        Button("Start sharing") { run { try await store.privatePrimaryStart(address: address) } }
                            .disabled(busy || address.isEmpty || status?.mode == "secondary" || status?.connected == true)
                        Button("Stop sharing") { run { try await store.privatePrimaryStop(); pairingCode = "" } }
                            .disabled(busy || status?.mode != "local" || status?.connected != true)
                    }
                    Button("Create pairing code") { run { pairingCode = try await store.privatePrimaryPairingCode() } }
                        .disabled(busy || status?.mode != "local" || status?.connected != true)
                    if !pairingCode.isEmpty {
                        Text(pairingCode).font(.title2.monospaced()).textSelection(.enabled)
                        Text("On the other computer, enter this primary’s address and code. Codes work once and expire in five minutes.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                } else {
                    Text("Connecting uses the selected primary’s Bots history. It does not move this Mac’s existing conversations or promote it to primary.")
                        .font(.caption).foregroundStyle(.secondary)
                    TextField("Primary address, e.g. http://192.168.1.10:8787", text: $endpoint).textFieldStyle(.roundedBorder)
                    TextField("Pairing code from the primary", text: $code).textFieldStyle(.roundedBorder)
                    Button("Connect and request approval") { run {
                        let value = try await store.privatePrimaryJoinBegin(endpoint: endpoint, code: code)
                        let details = try JSONDecoder().decode(Details.self, from: Data(value.utf8))
                        request = value; fingerprint = String(details.credential_sha256.prefix(12)); authority = details.authority_id; approval = ""
                    } }.disabled(busy || endpoint.isEmpty || code.isEmpty)
                    if !request.isEmpty {
                        Text("Computer fingerprint: \(fingerprint)").textSelection(.enabled)
                        Text("Primary identifier: \(authority)").font(.caption).textSelection(.enabled)
                        ShareLink("Copy or share approval request", item: request)
                        Link("Approve on the Hive website", destination: URL(string: "https://ohghive.com/private-fleet/enroll")!)
                        Text("Choose the same fleet used by the primary, compare these details, then paste your approval here.").font(.caption)
                        TextField("Approval", text: $approval, axis: .vertical).lineLimit(3...5).textFieldStyle(.roundedBorder)
                        Button("Use this primary") { run {
                            try await store.privatePrimaryJoinComplete(approval: approval)
                            request = ""; approval = ""; code = ""
                        } }.disabled(busy || approval.isEmpty)
                    }
                }
                if busy { ProgressView().controlSize(.small) }
                if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
                Button("Check connection") { run {} }.disabled(busy)
            }.frame(maxWidth: .infinity, alignment: .leading)
        }.task { run {} }
    }
    private func run(_ operation: @escaping @MainActor () async throws -> Void) {
        guard !busy else { return }
        busy = true; error = nil
        Task { @MainActor in
            defer { busy = false }
            do { try await operation(); status = try await store.privatePrimaryStatus() }
            catch { self.error = error.localizedDescription }
        }
    }
    private struct Details: Decodable { let credential_sha256: String; let authority_id: String }
}
