import SwiftUI
import OHHiveFFI

struct BotsView: View {
    @Bindable var model: BotsModel
    @State private var showsInspector = true
    @State private var showsNewRoom = false

    private var selected: BotsAgent? { model.agents.first { $0.id == model.selectedID } }
    private var canSend: Bool {
        let reachable = model.isRoom || (selected?.runtimeKind == "local" && (selected?.preferredHost == model.hostID || model.primaryEndpoint != nil))
        return reachable &&
            model.conversation != nil && !model.sending && !model.loading &&
            !model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && model.draft.utf8.count <= 65536
    }

    var body: some View {
        // Bots already lives inside the main NavigationSplitView. A nested native
        // split view can repeatedly invalidate the window's size constraints on
        // macOS 27 when this destination opens. Keep the inner columns stable.
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                Text("Your agents").font(.headline).padding(.horizontal)
                List(selection: $model.selectedID) {
                    Section("Rooms") {
                        ForEach(model.rooms, id: \.id) { room in
                            Label(room.title ?? "Room", systemImage: room.kind == "project" ? "folder" : "person.3").tag("room:" + room.id)
                        }
                    }
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
                Button("New room", systemImage: "person.3.sequence.fill") { showsNewRoom = true }
                    .disabled(!model.paired || model.agents.isEmpty).padding(.horizontal)
                Button("Register this Mac", systemImage: "plus") { Task { await model.register() } }
                    .disabled(!model.paired || model.registering).padding(.horizontal)
                Button("Refresh agents", systemImage: "arrow.clockwise") { Task { await model.refreshAgents() } }
                    .disabled(!model.paired).padding(.horizontal)
            }.padding(.vertical).frame(width: 220)
            Divider()
            VStack(spacing: 0) {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(model.roomTitle ?? selected?.name ?? "Bots").font(.title2)
                        Text(model.storageLabel).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Reconnect", systemImage: "arrow.triangle.2.circlepath") { Task { await model.reconnect() } }
                        .disabled(!model.paired)
                    Button("Agent details", systemImage: "sidebar.right") { showsInspector.toggle() }
                        .labelStyle(.iconOnly).help("Show or hide agent details")
                        .disabled(selected == nil)
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
                if selected == nil && !model.isRoom {
                    ContentUnavailableView("Chat with your agents", systemImage: "person.2.wave.2", description: Text(model.paired ? "Select an agent or register this Mac to begin." : "Connect this Mac in Settings first."))
                } else {
                    messages
                    if model.isRoom {
                        Text(model.roomAgents.filter { !$0.archived }.map(\.name).joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).padding(.horizontal)
                        Text("Address @names or @everyone for replies. Messages without mentions notify no agents.").font(.caption).foregroundStyle(.secondary).padding(.horizontal)
                        if let note = model.mentionNote { Text(note).font(.caption).foregroundStyle(.orange).padding(.horizontal) }
                    } else if selected?.preferredHost != model.hostID || selected?.runtimeKind != "local" {
                        Text(model.primaryEndpoint != nil && selected?.runtimeKind == "local" ? "Replies run on the agent’s computer. Secondary execution is not connected yet; messages stay queued on the primary." : "This agent cannot reply here yet. Choose a local agent on this Mac.")
                            .font(.callout).foregroundStyle(.secondary).padding()
                    }
                    Divider()
                    HStack(alignment: .bottom) {
                        TextField(model.isRoom ? "Message the room…" : "Message your agent…", text: Binding(get: { model.draft }, set: { model.draft = $0; model.saveDraft() }), axis: .vertical)
                            .lineLimit(1...6).textFieldStyle(.roundedBorder)
                            .accessibilityLabel("Message your agent")
                        Button(model.sending ? "Sending…" : "Send", systemImage: "arrow.up") { Task { await model.send() } }
                            .buttonStyle(.borderedProminent).disabled(!canSend)
                            .keyboardShortcut(.return, modifiers: .command)
                    }.padding()
                }
            }.frame(minWidth: 380, maxWidth: .infinity, maxHeight: .infinity)
        }
        .inspector(isPresented: Binding(get: { showsInspector && selected != nil }, set: { showsInspector = $0 })) {
            if let selected {
                BotsAgentInspector(agent: selected, model: model)
                    .id(selected.id)
                    .inspectorColumnWidth(min: 240, ideal: 280, max: 340)
            }
        }
        .sheet(isPresented: $showsNewRoom) { BotsNewRoomView(model: model) }
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
                        BotsMessageRow(message: message, agentName: model.authorName(message))
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
                Text(agentName).font(.headline)
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
