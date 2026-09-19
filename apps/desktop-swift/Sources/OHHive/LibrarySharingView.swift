import SwiftUI
import OHHiveFFI

/// Administration of collections stored on this Mac, never a remote-admin RPC.
struct LibrarySharingView: View {
    let collection: VaultInfo
    let store: HiveStore
    @Environment(\.dismiss) private var dismiss
    @State private var computers: [VaultComputerAccess] = []
    @State private var error: String?
    @State private var loaded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Computer access").font(.title2)
                Spacer()
                Button("Done") { dismiss() }
            }
            Text(collection.name).font(.headline)
            Text("Choose which paired computers can read this collection stored on this Mac. Agents also need access in their own Tools and access settings.")
            Text("Turning access off blocks future reads. It cannot remove information already read or included in conversations.")
                .font(.callout).foregroundStyle(.secondary)
            if let error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            if loaded && computers.isEmpty {
                ContentUnavailableView("No paired computers", systemImage: "desktopcomputer", description: Text("Connect your computers in Settings → Private Fleet, then return here."))
            } else {
                List(computers, id: \.nodeId) { computer in
                    VStack(alignment: .leading, spacing: 4) {
                        Toggle(isOn: Binding(get: { computer.allowed }, set: { setAccess(computer, allowed: $0) })) {
                            Text(computer.name)
                        }.disabled(!computer.active && !computer.allowed)
                        Text(computer.active ? "Paired computer" : "Pairing revoked — remove any saved access")
                            .font(.caption).foregroundStyle(.secondary)
                        // Display names need not be unique; allow the owner to distinguish them.
                        DisclosureGroup("Computer identity") {
                            Text(computer.nodeId).font(.caption.monospaced()).textSelection(.enabled)
                        }
                    }.padding(.vertical, 4)
                }
            }
            HStack {
                Text("Changes are saved immediately.").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Refresh") { load() }
            }
        }.padding(24).frame(width: 580, height: 480).onAppear { load() }
    }
    private func load() {
        do { computers = try store.vaultComputerAccess(vaultId: collection.id); loaded = true; error = nil }
        catch { self.error = error.localizedDescription }
    }
    private func setAccess(_ computer: VaultComputerAccess, allowed: Bool) {
        do {
            try store.vaultSetComputerAccess(vaultId: collection.id, nodeId: computer.nodeId, allowed: allowed)
            load()
        } catch { self.error = error.localizedDescription }
    }
}
