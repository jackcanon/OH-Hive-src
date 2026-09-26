import SwiftUI
import OHHiveFFI

/// Minimal read-only Handoff visibility view (source → target, task, state) -- the milestones
/// doc's "Immediate next actions" #2: before this, the GUI had zero Handoff surface anywhere.
/// Deliberately not an editor: creating and resolving handoffs still goes through
/// `hive hub handoff create/resolve` (CLI) or the chat tools an agent calls itself mid-chat
/// (Phase 2 slice 2) -- this view exists so a person can see what the fleet's agents are
/// actually handing to each other without reaching for a terminal.
struct HandoffsView: View {
    @Bindable var model: BotsModel

    private func agentName(_ id: String) -> String {
        model.agents.first { $0.id == id }?.name ?? String(id.prefix(8))
    }

    private func stateLabel(_ state: String) -> String {
        switch state {
        case "requested": return "Requested"
        case "accepted": return "Accepted"
        case "rejected": return "Rejected"
        case "in_progress": return "In progress"
        case "awaiting_correction": return "Awaiting correction"
        case "completed": return "Completed"
        case "failed": return "Failed"
        case "expired": return "Expired"
        default: return state.capitalized
        }
    }

    private func stateColor(_ state: String) -> Color {
        switch state {
        case "completed": return .green
        case "failed", "rejected", "expired": return .red
        case "in_progress", "accepted": return .blue
        case "awaiting_correction": return .orange
        default: return .secondary
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if model.loadingHandoffs && model.handoffs.isEmpty {
                    ProgressView("Loading handoffs…").frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if let error = model.handoffsError, model.handoffs.isEmpty {
                    ContentUnavailableView("Can’t load handoffs", systemImage: "arrow.triangle.branch", description: Text(error))
                } else if model.handoffs.isEmpty {
                    ContentUnavailableView("No handoffs yet", systemImage: "arrow.triangle.branch", description: Text("Handoffs created with `hive hub handoff create`, or by an agent that calls it mid-chat, will show up here."))
                } else {
                    List(model.handoffs, id: \.id) { handoff in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack(spacing: 6) {
                                Text(agentName(handoff.sourceAgent)).fontWeight(.medium)
                                Image(systemName: "arrow.right").foregroundStyle(.secondary)
                                Text(agentName(handoff.targetAgent)).fontWeight(.medium)
                                Spacer()
                                Text(stateLabel(handoff.state))
                                    .font(.caption).fontWeight(.semibold)
                                    .foregroundStyle(stateColor(handoff.state))
                            }
                            Text(handoff.taskOrQuestion).font(.body).lineLimit(3)
                            if let summary = handoff.receiptSummary, !summary.isEmpty {
                                Text(summary).font(.caption).foregroundStyle(.secondary).lineLimit(3)
                            }
                            Text(handoff.createdAt).font(.caption2).foregroundStyle(.tertiary)
                        }
                        .padding(.vertical, 4)
                    }
                }
            }
            .navigationTitle("Handoffs")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("Refresh", systemImage: "arrow.clockwise") { Task { await model.refreshHandoffs() } }
                        .disabled(model.loadingHandoffs)
                }
            }
            .task { await model.refreshHandoffs() }
        }
        .frame(minWidth: 480, minHeight: 420)
    }
}
