import SwiftUI
import OHHiveFFI

struct BotsView: View {
    @Bindable var model: BotsModel

    private var selected: BotsAgent? { model.agents.first { $0.id == model.selectedID } }
    private var canSend: Bool {
        guard let selected else { return false }
        return selected.runtimeKind == "local" && selected.preferredHost == model.hostID &&
            model.conversation != nil && !model.sending && !model.loading &&
            !model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && model.draft.utf8.count <= 65536
    }

    var body: some View {
        HSplitView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Your agents").font(.headline).padding(.horizontal)
                List(selection: $model.selectedID) {
                    ForEach(model.agents, id: \.id) { agent in
                        VStack(alignment: .leading, spacing: 3) {
                            Text(agent.name).lineLimit(2)
                            Text(agent.preferredHost == model.hostID ? "This Mac" : "Another computer")
                                .font(.caption).foregroundStyle(.secondary)
                        }.tag(agent.id)
                    }
                }
                if model.agents.isEmpty {
                    Text("Register this Mac to start a conversation with a local agent.")
                        .font(.callout).foregroundStyle(.secondary).padding(.horizontal)
                }
                Button("Register this Mac", systemImage: "plus") { Task { await model.register() } }
                    .disabled(!model.paired || model.registering).padding(.horizontal)
                Button("Refresh agents", systemImage: "arrow.clockwise") { Task { await model.refreshAgents() } }
                    .disabled(!model.paired).padding(.horizontal)
            }.padding(.vertical).frame(minWidth: 190, idealWidth: 220, maxWidth: 280)
            VStack(spacing: 0) {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(selected?.name ?? "Bots").font(.title2)
                        Text("Private conversations stored on this Mac").font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Reconnect", systemImage: "arrow.triangle.2.circlepath") { Task { await model.reconnect() } }
                        .disabled(!model.paired)
                }.padding()
                HStack {
                    Text(model.workerStatus).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    Spacer()
                }.padding(.horizontal).padding(.bottom, 8)
                Divider()
                if model.loading { ProgressView("Opening conversation…").padding() }
                if let error = model.sendError ?? model.error {
                    Text(error).font(.callout).foregroundStyle(.red).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading).padding()
                }
                if selected == nil {
                    ContentUnavailableView("Chat with your agents", systemImage: "person.2.wave.2", description: Text(model.paired ? "Select an agent or register this Mac to begin." : "Connect this Mac in Settings first."))
                } else {
                    messages
                    if selected?.preferredHost != model.hostID || selected?.runtimeKind != "local" {
                        Text("This agent cannot reply here yet. Choose a local agent on this Mac.")
                            .font(.callout).foregroundStyle(.secondary).padding()
                    }
                    Divider()
                    HStack(alignment: .bottom) {
                        TextField("Message your agent…", text: Binding(get: { model.draft }, set: { model.draft = $0; model.saveDraft() }), axis: .vertical)
                            .lineLimit(1...6).textFieldStyle(.roundedBorder)
                            .accessibilityLabel("Message your agent")
                        Button(model.sending ? "Sending…" : "Send", systemImage: "arrow.up") { Task { await model.send() } }
                            .buttonStyle(.borderedProminent).disabled(!canSend)
                            .keyboardShortcut(.return, modifiers: .command)
                    }.padding()
                }
            }.frame(minWidth: 380, maxWidth: .infinity, maxHeight: .infinity)
        }
        .navigationTitle("Bots")
        .task(id: model.paired) { if model.paired { await model.refreshAgents() } }
        .task(id: model.selectedID) { await model.watch(agentID: model.selectedID) }
    }

    private var messages: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    if model.messages.isEmpty && !model.loading {
                        Text("Say hello to start your conversation.").foregroundStyle(.secondary).padding()
                    }
                    ForEach(model.messages, id: \.id) { message in
                        BotsMessageRow(message: message, agentName: selected?.name ?? "Agent")
                            .id(message.id)
                    }
                }.padding()
            }
            .defaultScrollAnchor(.bottom)
            .onChange(of: model.messages.last?.id) {
                // Keep fresh replies visible; no animation avoids motion while polling.
                if let id = model.messages.last?.id { proxy.scrollTo(id, anchor: .bottom) }
            }
        }
    }
}

private struct BotsMessageRow: View {
    let message: BotsMessage
    let agentName: String
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(message.authorKind == "user" ? "You" : agentName).font(.headline)
                if let date = ISO8601DateFormatter().date(from: message.createdAt) {
                    Text(date, style: .time).font(.caption).foregroundStyle(.secondary)
                }
            }
            Text(message.body ?? "Message without text").textSelection(.enabled)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(message.authorKind == "user" ? Color.accentColor.opacity(0.08) : Color.secondary.opacity(0.07), in: .rect(cornerRadius: 12))
        .accessibilityElement(children: .combine)
    }
}
