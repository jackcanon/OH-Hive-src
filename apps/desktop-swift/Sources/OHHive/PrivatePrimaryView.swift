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
    @State private var connectionMessage: String?
    init(initialAction: String = "host") { _action = State(initialValue: initialAction) }
    private var working: Bool { busy || signIn.busy }
    private var target: String {
        manual ? endpoint.trimmingCharacters(in: .whitespacesAndNewlines) :
            discovery.computers.first(where: { $0.id == selectedComputer })?.endpoint ?? ""
    }
    private var approvalTarget: String? {
        manual ? nil : discovery.computers.first(where: { $0.id == selectedComputer })?.approvalEndpoint
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
                        Button("Make this Mac available") { run { try await store.privatePrimaryStartNearby() } }
                            .disabled(working || status?.mode == "secondary" || status?.connected == true)
                        if status?.mode == "local" && status?.connected == true {
                            Button("Stop sharing") { run { try await store.privatePrimaryStop(); pairingCode = "" } }
                        }
                    }
                    if status?.mode == "local" && status?.connected == true {
                        FleetDiscoveryNotice(advertisement: store.fleetAdvertisement)
                        Text("Ready for your other computers. Choose this Mac there; approve the request below when it appears.")
                            .font(.callout)
                        FleetPairingRequests(pairing: store.fleetAdvertisement.pairing)
                    }
                    DisclosureGroup("Advanced network settings") {
                        TextField("Listen address and port", text: $address).textFieldStyle(.roundedBorder)
                        Button("Share using this address") { run { try await store.privatePrimaryStart(address: address) } }
                            .disabled(working || address.isEmpty || status?.mode == "secondary" || status?.connected == true)
                        if let endpoint = status?.endpoint { Text(endpoint).font(.caption).textSelection(.enabled) }
                        if status?.mode == "local" && status?.connected == true {
                            Button("Create a manual pairing code") { run { pairingCode = try await store.privatePrimaryPairingCode() } }.disabled(working)
                            if !pairingCode.isEmpty { Text(pairingCode).font(.title2.monospaced()).textSelection(.enabled) }
                        }
                    }
                } else {
                    Text("Choose your primary computer, then request to join. Approve the request on that computer; sign-in finishes automatically in your browser.")
                        .font(.callout).foregroundStyle(.secondary)
                    if discovery.computers.isEmpty {
                        Text("Looking for nearby computers…").font(.headline)
                        Text("On your primary, open Private Fleet and choose Make this Mac available. Both computers need to be on the same network.")
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
                    if target.isEmpty {
                        Text("Choose your primary above to continue.").font(.callout)
                    } else if approvalTarget == nil {
                        if !manual {
                            Text("This primary needs an app update for automatic pairing. Update it, or use a manual pairing code from Advanced network settings there.")
                                .font(.callout).foregroundStyle(.secondary)
                        }
                        TextField("Manual pairing code", text: $code).textFieldStyle(.roundedBorder).disabled(working)
                    } else {
                        Text("No code needed. Click Request to join, then approve this computer on your primary.").font(.callout)
                    }
                    if signIn.busy {
                        ProgressView(signIn.progress)
                        Button("Cancel") { signIn.cancel() }
                    } else {
                        Button(approvalTarget == nil ? "Join and sign in" : "Request to join") { signIn.start(store: store, primaryEndpoint: target, pairingCode: code.trimmingCharacters(in: .whitespacesAndNewlines), approvalEndpoint: approvalTarget) }
                            .buttonStyle(.borderedProminent)
                            .disabled(working || target.isEmpty || (approvalTarget == nil && code.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty))
                    }
                    if let message = signIn.message { Text(message).font(.callout).textSelection(.enabled) }
                }
                if busy { ProgressView().controlSize(.small) }
                if let error { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
                if status?.mode == "secondary" { PrivateCodingWorkerControls(worker: store.codingWorker) }
                Button("Check connection") { run(reportStatus: true) {} }.disabled(working)
                if let connectionMessage { Text(connectionMessage).font(.callout).textSelection(.enabled) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
        .task { discovery.start(); run {} }
        .onDisappear { discovery.stop(); signIn.cancel() }
        .onChange(of: discovery.computers) {
            if selectedComputer.isEmpty && discovery.computers.count == 1 {
                selectedComputer = discovery.computers[0].id
            }
        }
        .onChange(of: signIn.busy) { if !signIn.busy && signIn.completed { code = ""; run {} } }
    }
    private func run(reportStatus: Bool = false, _ operation: @escaping @MainActor () async throws -> Void) {
        guard !working else { return }
        busy = true; error = nil; connectionMessage = nil
        Task { @MainActor in
            defer { busy = false }
            do {
                try await operation()
                let checked = try await store.privatePrimaryStatus()
                status = checked
                if reportStatus {
                    if checked.mode == "secondary" {
                        connectionMessage = checked.connected ? "Connected to your primary. Open Bots to see its agents." : "Your primary is unavailable. Check that Loki’s Den is open and sharing there."
                    } else {
                        connectionMessage = checked.connected ? "This Mac is sharing as a primary." : "This Mac has not joined a primary yet. Choose your primary computer and click Request to join."
                    }
                }
            }
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


private struct FleetPairingRequests: View {
    @ObservedObject var pairing: FleetPairingApproval
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(pairing.requests) { request in
                GroupBox("Connection request") {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("\(request.name) wants to join your fleet.").font(.headline)
                        Text("Approve only if you just requested this from your other computer. The account must still match your fleet.")
                            .font(.caption).foregroundStyle(.secondary)
                        HStack {
                            Button(request.issuing ? "Approving…" : "Approve") { Task { await pairing.approve(request.id) } }
                                .buttonStyle(.borderedProminent).disabled(request.issuing)
                            Button("Decline") { pairing.decline(request.id) }.disabled(request.issuing)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            if let error = pairing.error { Text(error).font(.caption).foregroundStyle(.red) }
        }
    }
}
