import SwiftUI

/// ADR-021 §4: remote checkout is fully real (the lease reaper already handles a quieted node
/// gracefully); remote check-*in* is intentionally not offered here -- a dormant machine's own app
/// has to be the one to actually start its worker. The UI says "Request check-in" rather than
/// implying a toggle, per the ADR's explicit call to not let this read as broken.
struct NodesView: View {
    @State private var nodes: [MemberNode] = []
    @State private var error: String?
    @State private var busyNodeId: String?

    var body: some View {
        List {
            if let error { Text(error).font(.caption).foregroundStyle(.secondary) }
            ForEach(nodes) { node in
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Circle().fill(color(for: node.presence)).frame(width: 8, height: 8)
                        Text(node.displayName).font(.body.weight(.medium))
                        Spacer()
                        Text(node.presence).font(.caption2).foregroundStyle(.secondary)
                    }
                    Text("\(node.role) \u{00b7} \(node.region)").font(.caption).foregroundStyle(.secondary)
                    HStack {
                        if node.presence != "checked_out" {
                            Button("Check out") { Task { await checkout(node) } }
                                .font(.caption)
                                .disabled(busyNodeId == node.id)
                        } else {
                            Text("Request check-in from the machine itself, or the Mac/Tauri app once you're there.")
                                .font(.caption2).foregroundStyle(.tertiary)
                        }
                    }
                }
            }
        }
        .navigationTitle("Servers & Agents")
        .task { await load() }
        .refreshable { await load() }
    }

    private func color(for presence: String) -> Color {
        switch presence {
        case "checked_in": return .green
        case "draining": return .orange
        default: return .secondary
        }
    }

    private func load() async {
        do {
            nodes = try await supabase.rpc("hive_member_nodes").execute().value
            error = nil
        } catch {
            self.error = "Couldn't load your nodes (\(error.localizedDescription))."
        }
    }

    private func checkout(_ node: MemberNode) async {
        busyNodeId = node.id
        defer { busyNodeId = nil }
        do {
            _ = try await supabase.rpc("hive_member_node_checkout", params: ["p_node_id": node.id]).execute()
            await load()
        } catch {
            self.error = "Couldn't check that node out (\(error.localizedDescription))."
        }
    }
}
