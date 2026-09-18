import SwiftUI
import OHHiveFFI

/// Endpoint selection only. Moving a primary's data and execution leases is a later handover flow.
struct PrivatePrimaryView: View {
    @EnvironmentObject private var store: HiveStore
    @StateObject private var discovery = PrivateFleetDiscovery()
    @StateObject private var signIn = PrivateFleetSignIn()
    @State private var status: PrivatePrimaryStatus?
    @State private var action: String
    @State private var address = ""
    @State private var endpoint = ""
    @State private var selectedComputer = ""
    @State private var code = ""
    @State private var pairingCode = ""
    @State private var manual = false
    @State private var busy = false
    @State private var error: String?
    init(initialAction: String = "host") { _action = State(initialValue: initialAction) }
    private var working: Bool { busy || signIn.busy }
    private var target: String {
        manual ? endpoint.trimmingCharacters(in: .whitespacesAndNewlines) :
            discovery.computers.first(where: { $0.id == selectedComputer })?.endpoint ?? ""
    }
    var body: some View {
        GroupBox("Your private fleet") {
            VStack(alignment: .leading, spacing: 12) {
                if let status {
                    Label(status.mode == "secondary" ? "Connected computer" : "This Mac’s workspace", systemImage: "desktopcomputer")
                    Text(status.detail).font(.caption).foregroundStyle(.secondary)
                }
                Picker("Set up your fleet", selection: $action) {
                    Text("Use this Mac as primary").tag("host")
                    Text("Join an existing fleet").tag("join")
                }.disabled(working)
                if action == "host" {
                    Text("Share this Mac’s agents with your other computers. Keep Loki’s Den open here while they’re connected.")
                        .font(.callout).foregroundStyle(.secondary)
                    HStack {
                        Button("Make this Mac discoverable") { run { try await store.privatePrimaryStartNearby() } }
                            .disabled(working || status?.mode == "secondary" || status?.connected == true)
                        if status?.mode == "local" && status?.connected == true {
                            Button("Stop sharing") { run { try await store.privatePrimaryStop(); pairingCode = "" } }
                        }
                    }
                    if status?.mode == "local" && status?.connected == true {
                        FleetDiscoveryNotice(advertisement: store.fleetAdvertisement)
                        Button("Create pairing code") { run { pairingCode = try await store.privatePrimaryPairingCode() } }
                            .disabled(working)
                        if !pairingCode.isEmpty {
                            Text(pairingCode).font(.title2.monospaced()).textSelection(.enabled)
                            Text("On your other computer, choose this Mac by name and enter this code. It works once and expires in five minutes.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    DisclosureGroup("Advanced network settings") {
                        TextField("Listen address and port", text: $address).textFieldStyle(.roundedBorder)
                        Button("Share using this address") { run { try await store.privatePrimaryStart(address: address) } }
                            .disabled(working || address.isEmpty || status?.mode == "secondary" || status?.connected == true)
                        if let endpoint = status?.endpoint { Text(endpoint).font(.caption).textSelection(.enabled) }
                    }
                } else {
                    Text("Choose your primary computer. Its owner approves this connection with a pairing code and sign-in.")
                        .font(.callout).foregroundStyle(.secondary)
                    if discovery.computers.isEmpty {
                        Text("Looking for nearby computers…").font(.headline)
                        Text("On your primary, open Private Fleet and choose Make this Mac discoverable. Both computers need to be on the same network.")
                            .font(.caption).foregroundStyle(.secondary)
                    } else {
                        Picker("Nearby primary", selection: $selectedComputer) {
                            Text("Choose a computer").tag("")
                            ForEach(discovery.computers) { computer in Text(computer.name).tag(computer.id) }
                        }.disabled(working || manual)
                        Text("Nearby names identify connection candidates. Sign-in verifies the fleet before any agents are shared.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if let message = discovery.message { Text(message).font(.caption).foregroundStyle(.secondary) }
                    Button("Look again") { discovery.stop(); discovery.start() }.disabled(working)
                    DisclosureGroup("Advanced: enter an address") {
                        Toggle("Use a manual address", isOn: $manual).disabled(working)
                        if manual { TextField("Primary address", text: $endpoint).textFieldStyle(.roundedBorder).disabled(working) }
                    }
                    TextField("Pairing code shown on your primary", text: $code).textFieldStyle(.roundedBorder).disabled(working)
                    if signIn.busy {
                        ProgressView("Finish approval in your browser…")
                        Button("Cancel") { signIn.cancel() }
                    } else {
                        Button("Join and sign in") { signIn.start(store: store, primaryEndpoint: target, pairingCode: code.trimmingCharacters(in: .whitespacesAndNewlines)) }
                            .buttonStyle(.borderedProminent)
                            .disabled(working || target.isEmpty || code.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    if let message = signIn.message { Text(message).font(.callout).textSelection(.enabled) }
                }
                if busy { ProgressView().controlSize(.small) }
                if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
                if status?.mode == "secondary" { PrivateCodingWorkerControls(worker: store.codingWorker) }
                Button("Check connection") { run {} }.disabled(working)
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        .task { discovery.start(); run {} }
        .onDisappear { discovery.stop(); signIn.cancel() }
        .onChange(of: signIn.busy) { if !signIn.busy && signIn.completed { code = ""; run {} } }
    }
    private func run(_ operation: @escaping @MainActor () async throws -> Void) {
        guard !working else { return }
        busy = true; error = nil
        Task { @MainActor in
            defer { busy = false }
            do { try await operation(); status = try await store.privatePrimaryStatus() }
            catch { self.error = error.localizedDescription }
        }
    }
}

private struct FleetDiscoveryNotice: View {
    @ObservedObject var advertisement: PrivateFleetAdvertisement
    var body: some View {
        if let message = advertisement.message { Text(message).font(.caption).foregroundStyle(.secondary) }
    }
}


private struct PrivateCodingWorkerControls: View {
    @ObservedObject var worker: PrivateCodingWorkerModel
    @EnvironmentObject private var github: GitHubAuthManager
    var body: some View {
        GroupBox("Coding on this computer") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Allow your primary to send coding tasks here. Uses this Mac’s model, memory and GitHub connection while Loki’s Den is open. Enable coding tools in Settings first.")
                    .font(.caption).foregroundStyle(.secondary)
                Text(worker.status).font(.caption).textSelection(.enabled)
                if worker.enabled {
                    Button("Stop coding worker") { worker.stop() }
                } else {
                    Button("Start coding worker on this Mac") { worker.start(github: github) }
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
