import SwiftUI
import OHHiveFFI

/// Read-only evidence of work agents have handed to teammates.
struct HandoffsView: View {
    @Bindable var model: BotsModel
    @State private var search = ""

    private var visibleHandoffs: [BotsHandoff] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return model.handoffs }
        return model.handoffs.filter { handoff in
            [agentName(handoff.sourceAgent), agentName(handoff.targetAgent),
             handoff.taskOrQuestion, handoff.acceptanceCriteria, stateLabel(handoff.state),
             handoff.receiptSummary ?? ""].contains { $0.localizedStandardContains(query) }
        }
    }

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
            VStack(spacing: 0) {
                if let error = model.handoffsError, !model.handoffs.isEmpty {
                    Label("Couldn’t refresh. Showing the last loaded handoffs. \(error)", systemImage: "exclamationmark.triangle")
                        .font(.caption).foregroundStyle(.orange).padding()
                }
                if model.loadingHandoffs && model.handoffs.isEmpty {
                    ProgressView("Loading handoffs…").frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if let error = model.handoffsError, model.handoffs.isEmpty {
                    ContentUnavailableView("Can’t load handoffs", systemImage: "arrow.triangle.branch", description: Text(error))
                } else if model.handoffs.isEmpty {
                    ContentUnavailableView("No handoffs yet", systemImage: "arrow.triangle.branch", description: Text("Ask an agent to hand work to an allowed teammate. Its request, acceptance criteria and result will appear here. Teammate access is managed in the agent’s tool settings."))
                } else if visibleHandoffs.isEmpty {
                    ContentUnavailableView.search(text: search)
                } else {
                    List(visibleHandoffs, id: \.id) { handoff in
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
                            DisclosureGroup("Task, acceptance and result") {
                                VStack(alignment: .leading, spacing: 8) {
                                    Text("Task").font(.caption).fontWeight(.semibold)
                                    Text(handoff.taskOrQuestion).textSelection(.enabled)
                                    Text("Acceptance criteria").font(.caption).fontWeight(.semibold)
                                    Text(handoff.acceptanceCriteria).textSelection(.enabled)
                                    if let summary = handoff.receiptSummary, !summary.isEmpty {
                                        Text("Saved result").font(.caption).fontWeight(.semibold)
                                        Text(summary).textSelection(.enabled)
                                    } else {
                                        Text("No result has been saved yet.").foregroundStyle(.secondary)
                                    }
                                    Text("Deadline: \(handoff.deadline)").font(.caption).foregroundStyle(.secondary)
                                }.frame(maxWidth: .infinity, alignment: .leading)
                            }
                            Text(handoff.createdAt).font(.caption2).foregroundStyle(.tertiary)
                        }
                        .padding(.vertical, 4)
                    }
                }
            }
            .navigationTitle("Handoffs")
            .searchable(text: $search, prompt: "Search tasks, teammates or results")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button("Refresh", systemImage: "arrow.clockwise") { Task { await model.refreshHandoffs() } }
                        .disabled(model.loadingHandoffs)
                }
            }
            .task {
                while !Task.isCancelled {
                    if !model.loadingHandoffs { await model.refreshHandoffs() }
                    do { try await Task.sleep(for: .seconds(5)) } catch { break }
                }
            }
        }
        .frame(minWidth: 480, minHeight: 420)
    }
}
