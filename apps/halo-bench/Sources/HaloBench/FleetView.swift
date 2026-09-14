import SwiftUI

/// The lab machines. Wired IPs and *measured* usable memory -- the two numbers Test 01 proved
/// matter more than the spec sheet.
struct FleetView: View {
    @Environment(BenchStore.self) private var store
    @State private var selected: Host.ID?
    @State private var probing: [Host.ID: Bool] = [:]
    @State private var reach: [Host.ID: Bool] = [:]

    var body: some View {
        @Bindable var store = store
        HSplitView {
            List(selection: $selected) {
                ForEach(store.config.fleet) { h in
                    HostRow(host: h, isLocal: h.id == store.config.localHostID, reachable: reach[h.id], probing: probing[h.id] == true)
                        .tag(h.id)
                }
            }
            .frame(minWidth: 320)
            .toolbar {
                Button { probeAll() } label: { Label("Probe RPC ports", systemImage: "dot.radiowaves.left.and.right") }
                Button { store.config.fleet.append(Host(name: "New host", wiredIP: "", sshUser: "jack", chip: "", ramGB: 16, usableGB: 10, backend: .metal, rpcServerPath: "~/halo/bin/ggml-rpc-server")) } label: { Label("Add", systemImage: "plus") }
            }

            if let idx = store.config.fleet.firstIndex(where: { $0.id == selected }) {
                HostEditor(host: $store.config.fleet[idx], isLocal: Binding(
                    get: { store.config.localHostID == store.config.fleet[idx].id },
                    set: { store.config.localHostID = $0 ? store.config.fleet[idx].id : nil }),
                    onDelete: { store.config.fleet.remove(at: idx); selected = nil })
                .frame(minWidth: 420)
            } else {
                ContentUnavailableView("Select a host", systemImage: "server.rack").frame(minWidth: 420)
            }
        }
        .navigationTitle("Fleet")
    }

    private func probeAll() {
        for h in store.config.fleet where !h.wiredIP.isEmpty {
            probing[h.id] = true
            Task {
                let ok = await Runner.probe(host: h.wiredIP, port: h.rpcPort)
                reach[h.id] = ok; probing[h.id] = false
            }
        }
    }
}

struct HostRow: View {
    var host: Host
    var isLocal: Bool
    var reachable: Bool?
    var probing: Bool

    private var dot: Color {
        guard let r = reachable else { return .gray }
        return r ? .green : .red
    }
    private var subtitle: String {
        let ip = host.wiredIP.isEmpty ? "no wired IP" : host.wiredIP
        return "\(host.chip) · \(ReportStore.num(host.ramGB)) GB · usable ~\(ReportStore.num(host.usableGB)) GB · \(ip)"
    }

    var body: some View {
        HStack {
            Circle().fill(dot).frame(width: 8, height: 8)
            VStack(alignment: .leading) {
                HStack {
                    Text(host.name).font(.headline)
                    if isLocal {
                        Text("this Mac").font(.caption2).padding(.horizontal, 6)
                            .background(Color.honey.opacity(0.25), in: Capsule())
                    }
                }
                Text(subtitle).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if probing { ProgressView().controlSize(.mini) }
        }
    }
}

struct HostEditor: View {
    @Binding var host: Host
    @Binding var isLocal: Bool
    var onDelete: () -> Void
    var body: some View {
        Form {
            Section("Machine") {
                TextField("Name", text: $host.name)
                TextField("Chip / GPU", text: $host.chip)
                Picker("Backend", selection: $host.backend) { ForEach(Host.Backend.allCases, id: \.self) { Text($0.rawValue).tag($0) } }
                TextField("RAM (GB)", value: $host.ramGB, format: .number)
                TextField("Usable GPU memory (GB, measured)", value: $host.usableGB, format: .number)
                Toggle("This is the Mac HaloBench runs on (the host)", isOn: $isLocal)
            }
            Section("Network — wired LAN IP, or Tailscale IP for remote machines") {
                TextField("Reachable IP (wired LAN or Tailscale 100.x)", text: $host.wiredIP)
                Toggle("Managed over SSH (off = a volunteer's machine; they start the worker themselves)", isOn: $host.managed)
                TextField("SSH user", text: $host.sshUser)
                TextField("RPC port", value: $host.rpcPort, format: .number)
                TextField("ggml-rpc-server path on that machine", text: $host.rpcServerPath)
                TextField("Worker device (-d), blank = MTL0 / CUDA0 by backend", text: Binding(get: { host.rpcDevice ?? "" }, set: { host.rpcDevice = $0.isEmpty ? nil : $0 }))
                TextField("llama-bench path on that machine (when it hosts a run)", text: Binding(get: { host.llamaBenchPath ?? "" }, set: { host.llamaBenchPath = $0.isEmpty ? nil : $0 }))
            }
            Section("Notes") {
                TextField("Notes", text: $host.notes, axis: .vertical).lineLimit(3...8)
            }
            Section {
                Button("Remove host", role: .destructive, action: onDelete)
            }
        }
        .formStyle(.grouped)
    }
}
