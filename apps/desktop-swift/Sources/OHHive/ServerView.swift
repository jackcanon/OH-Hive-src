import SwiftUI
import OHHiveFFI
import AppKit

/// Bundled `cloudflared` inside the app's own Resources -- assembled by `scripts/build-app.sh`
/// from the same binary the Tauri app's build.rs fetches (ADR-013 D74). `nil` if this build has
/// none (e.g. it wasn't copied in), matching the Tauri app's `tunnel::bundled_path`.
private func bundledCloudflaredPath() -> String? {
    guard let dir = Bundle.main.resourceURL else { return nil }
    let path = dir.appendingPathComponent("cloudflared").path
    return FileManager.default.fileExists(atPath: path) ? path : nil
}

/// Regional-server role + Cloudflare Tunnel (ADR-018 tasks #70/#71). Mirrors
/// `apps/desktop/src/Server.tsx` -- status, guided Reachability (bundled tunnel login -> create
/// -> done, or a manually-configured URL if this build has no bundled binary), and Offer.
struct ServerView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var url: String = ""
    @State private var storageGb: Double = 50
    @State private var tier: String = "primary"
    @State private var dataDir: String = ""
    @State private var tunnelName: String = ""
    @State private var tunnelBusy = false
    @State private var tunnelError: String?
    @State private var pollTask: Task<Void, Never>?

    var body: some View {
        ScrollView {
            if store.snapshot?.paired != true {
                Text("Pair this machine first (Node section).")
                    .foregroundStyle(.secondary)
                    .padding(20)
            } else if let sv = store.server {
                VStack(alignment: .leading, spacing: 14) {
                    statusCard(sv)
                    reachabilityCard(sv)
                    offerCard(sv)
                }
                .padding(20)
                .onAppear {
                    url = sv.publicUrl
                    storageGb = Double(sv.storageGb)
                    tier = sv.tier
                    dataDir = sv.dataDir
                    if tunnelName.isEmpty { tunnelName = suggestedTunnelName() }
                }
            } else {
                ProgressView().padding(20)
            }
        }
        .navigationTitle("Server")
        .task {
            store.refresh()
            pollTask = Task {
                while !Task.isCancelled {
                    try? await Task.sleep(nanoseconds: 4_000_000_000)
                    store.refresh()
                }
            }
        }
        .onDisappear { pollTask?.cancel() }
    }

    @ViewBuilder private func statusCard(_ sv: ServerInfo) -> some View {
        GroupBox {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(sv.running ? (sv.registered ? Color.green : Color.orange) : Color.secondary.opacity(0.4))
                            .frame(width: 8, height: 8)
                        Text(sv.running ? (sv.registered ? "Serving the Hive" : "Starting\u{2026}") : "Regional server off")
                            .fontWeight(.semibold)
                    }
                    Text(sv.running
                         ? "\(sv.coordinator ? "coordinator of the Hive" : (sv.coordinatorName.map { "follower \u{00b7} coordinator is \($0)" } ?? "follower")) \u{00b7} \(sv.blobs) blob\(sv.blobs == 1 ? "" : "s"), \(gb(sv.usedBytes))"
                         : "Holds artifacts, relays live boards, competes for coordinator. Earns Honey for bytes stored and served.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if sv.running {
                    Button("Stop") { Task { await store.serverStop() } }
                } else {
                    Button("Start serving") {
                        Task { await store.serverStart(publicUrl: url, storageGb: UInt32(storageGb), tier: tier) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(url.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            if let backup = sv.lastBackup {
                Text("Last hub backup taken here: \(String(backup.prefix(12)))\u{2026}")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            if !sv.running {
                Text("Before you start: check the Offer settings below (disk size, tier, storage location) \u{2014} they lock once the server is running.")
                    .font(.caption2).foregroundStyle(.secondary)
            }
        }
    }

    @ViewBuilder private func reachabilityCard(_ sv: ServerInfo) -> some View {
        let bin = bundledCloudflaredPath()
        let tn = store.tunnel

        GroupBox("Reachability") {
            VStack(alignment: .leading, spacing: 8) {
                if let error = tunnelError {
                    Text(error).font(.caption).foregroundStyle(.red)
                }

                if bin == nil {
                    manualUrlFields(sv)
                } else if let tn, !tn.loggedIn {
                    Text("A free Cloudflare Tunnel gives this machine a public HTTPS address \u{2014} no port forwarding, no certificates. Connect your Cloudflare account and the app does the rest.")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("Connect Cloudflare") {
                        Task { await connectCloudflare(bin: bin!) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(tunnelBusy)
                    Text("Opens in your browser. Come back here once you've signed in.")
                        .font(.caption2).foregroundStyle(.secondary)
                } else if let tn, tn.loggedIn, tn.hostname == nil {
                    Text("Cloudflare connected. Pick a short name for this machine \u{2014} it becomes its address.")
                        .font(.caption).foregroundStyle(.secondary)
                    HStack {
                        TextField("vanaheim", text: $tunnelName)
                            .textFieldStyle(.roundedBorder)
                            .onChange(of: tunnelName) { _, v in
                                tunnelName = v.lowercased().filter { $0.isLetter || $0.isNumber || $0 == "-" }
                            }
                        Text(".ohghive.com").font(.caption).foregroundStyle(.secondary)
                    }
                    Button("Create tunnel") {
                        Task { await createTunnel(bin: bin!) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(tunnelBusy || tunnelName.isEmpty)
                } else if let tn, let hostname = tn.hostname {
                    HStack(spacing: 6) {
                        Circle().fill(tn.running ? Color.green : Color.secondary.opacity(0.4)).frame(width: 8, height: 8)
                        Text("https://\(hostname)").font(.system(size: 13, design: .monospaced))
                        Spacer()
                        Text(tn.running ? "connected" : (sv.running ? "connecting\u{2026}" : "starts with the server"))
                            .font(.caption2).foregroundStyle(.secondary)
                    }
                    Text("This is what members and other servers use to reach this machine.")
                        .font(.caption2).foregroundStyle(.secondary)
                } else {
                    ProgressView().controlSize(.small)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder private func manualUrlFields(_ sv: ServerInfo) -> some View {
        Text("Public URL (how members and other servers reach this machine)")
            .font(.caption).foregroundStyle(.secondary)
        TextField("https://yourname.ohghive.com", text: $url)
            .textFieldStyle(.roundedBorder)
            .disabled(sv.running)
        Text("This build has no bundled Cloudflare Tunnel \u{2014} set one up in Terminal:")
            .font(.caption2).foregroundStyle(.secondary)
        Text("cloudflared tunnel login\ncloudflared tunnel create <name>\ncloudflared tunnel route dns <name> <name>.ohghive.com")
            .font(.system(size: 11, design: .monospaced))
            .foregroundStyle(.secondary)
        Text("Point it at localhost:8790, then paste the hostname above.")
            .font(.caption2).foregroundStyle(.secondary)
        Link("Guide", destination: URL(string: "https://github.com/jackcanon/ohhive-releases/blob/main/README.md")!)
            .font(.caption)
    }

    private func connectCloudflare(bin: String) async {
        tunnelBusy = true
        tunnelError = nil
        await store.tunnelLogin(binPath: bin)
        tunnelBusy = false
        store.refresh()
    }

    /// Mirrors `Server.tsx`'s `suggestedName`: derive a default tunnel name from this node's
    /// paired display name, if known.
    private func suggestedTunnelName() -> String {
        guard let json = store.snapshot?.summaryJson,
              let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let node = obj["node"] as? [String: Any],
              let raw = node["display_name"] as? String
        else { return "" }
        let filtered = raw.lowercased().filter { $0.isLetter || $0.isNumber || $0 == "-" }
        return String(filtered.prefix(24))
    }

    private func createTunnel(bin: String) async {
        tunnelBusy = true
        tunnelError = nil
        do {
            let publicUrl = try await store.tunnelSetup(binPath: bin, name: tunnelName, hostname: "\(tunnelName).ohghive.com")
            url = publicUrl
        } catch {
            tunnelError = String(describing: error)
        }
        tunnelBusy = false
        store.refresh()
    }

    @ViewBuilder private func offerCard(_ sv: ServerInfo) -> some View {
        GroupBox("Offer") {
            VStack(alignment: .leading, spacing: 10) {
                Text("Worth setting up now \u{2014} disk size, tier, and storage location all lock once the server is running; changing any of them later means stopping it first.")
                    .font(.caption2).foregroundStyle(.secondary)

                Text("Disk to offer: \(Int(storageGb)) GB")
                    .font(.caption)
                Slider(value: $storageGb, in: 20...4000, step: 10)
                    .disabled(sv.running)

                Text("Tier").font(.caption)
                Picker("", selection: $tier) {
                    Text("Primary \u{2014} always on, first choice for relay and storage").tag("primary")
                    Text("Standby \u{2014} backups and overflow only").tag("standby")
                }
                .labelsHidden()
                .disabled(sv.running)

                Text("Storage location").font(.caption)
                HStack {
                    Text(dataDir)
                        .font(.system(size: 11, design: .monospaced))
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer()
                    Button("Choose folder\u{2026}") { pickStorageFolder(sv) }
                        .disabled(sv.running)
                }
                Text("Listening on \(sv.listen). Operator: \(sv.operatorName).")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func pickStorageFolder(_ sv: ServerInfo) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        if !dataDir.isEmpty {
            panel.directoryURL = URL(fileURLWithPath: dataDir)
        }
        guard panel.runModal() == .OK, let picked = panel.url?.path else { return }
        if sv.blobs > 0 && picked != sv.dataDir {
            let alert = NSAlert()
            alert.messageText = "Switch storage location?"
            alert.informativeText = "This machine already holds \(sv.blobs) blob\(sv.blobs == 1 ? "" : "s") (\(gb(sv.usedBytes))) at \(sv.dataDir). Switching won't move them \u{2014} they'll stay there, unreachable, until you point storage back at that folder. Continue?"
            alert.addButton(withTitle: "Continue")
            alert.addButton(withTitle: "Cancel")
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        do {
            _ = try store.setDataDir(picked)
            dataDir = picked
        } catch {
            store.lastError = String(describing: error)
        }
    }
}

private func gb(_ bytes: UInt64) -> String {
    let v = Double(bytes) / 1_073_741_824
    let decimals = v > 10 ? 0 : 2
    return String(format: "%.\(decimals)f GB", v)
}
