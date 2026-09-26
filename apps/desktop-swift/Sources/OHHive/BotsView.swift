import SwiftUI
import OHHiveFFI

struct BotsView: View {
    @Bindable var model: BotsModel
    @State private var showsInspector = false
    @State private var showsAgents = false
    @State private var showsNewRoom = false
    @State private var showsTeamStarter = false
    @State private var showsHandoffs = false

    private var selected: BotsAgent? { model.agents.first { $0.id == model.selectedID } }
    private var canSend: Bool {
        let reachable = model.isRoom || BotsModel.canMessageAgent(selected)
        return reachable &&
            model.conversation != nil && !model.sending && !model.loading &&
            !model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && model.draft.utf8.count <= 65536
    }

    var body: some View {
        // Derive the columns from available space; never feed measured sizes back
        // into window constraints or nest native split views.
        GeometryReader { geometry in
            HStack(spacing: 0) {
                if geometry.size.width >= 700 {
                    agentList.frame(width: 220)
                    Divider()
                }
                VStack(spacing: 0) {
                    if geometry.size.width < 700 {
                        HStack {
                            Button("Agents and rooms", systemImage: "person.2") { showsAgents = true }
                                .popover(isPresented: $showsAgents) {
                                    agentList.frame(width: 260, height: 440)
                                }
                            Spacer()
                        }.padding([.horizontal, .top])
                    }
                    conversation
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .inspector(isPresented: Binding(get: { showsInspector && selected != nil }, set: { showsInspector = $0 })) {
            if let selected {
                BotsAgentInspector(agent: selected, model: model)
                    .id(selected.id)
                    .inspectorColumnWidth(min: 260, ideal: 280, max: 340)
            }
        }
        .sheet(isPresented: $showsTeamStarter) { TeamStarterView(model: model) }
        .sheet(isPresented: $showsNewRoom) { BotsNewRoomView(model: model) }
        .sheet(isPresented: $showsHandoffs) { HandoffsView(model: model) }
        .navigationTitle("Bots")
        .task(id: model.paired) { if model.paired { await model.refreshAgents() } }
        .task(id: model.selectedID) { await model.watch(agentID: model.selectedID) }
        .onChange(of: model.selectedID) { showsAgents = false; showsInspector = selected != nil }
    }

    private var agentList: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Your agents").font(.headline).padding(.horizontal)
            List(selection: $model.selectedID) {
                Section("Rooms") {
                    ForEach(model.rooms, id: \.id) { room in
                        Label(room.title ?? "Room", systemImage: room.kind == "project" ? "folder" : "person.3").tag("room:" + room.id)
                    }
                }
                ForEach(model.agents, id: \.id) { agent in
                    HStack {
                        AgentAvatar(name: model.biographies[agent.id]?.avatar ?? "", size: 32)
                        VStack(alignment: .leading, spacing: 3) {
                        Text(agent.name).lineLimit(2)
                        Text(agent.runtimeKind == "local" ? (agent.preferredHost == model.hostID ? "This Mac" : "Fleet agent") : "Cloud agent")
                            .font(.caption).foregroundStyle(.secondary)
                        }
                    }.tag(agent.id)
                }
            }
            if model.agents.isEmpty {
                Text("Register this Mac to start a conversation with a local agent.")
                    .font(.callout).foregroundStyle(.secondary).padding(.horizontal)
            }
            Button("Build a team…", systemImage: "person.3.fill") { showsTeamStarter = true }
                .disabled(!model.paired || model.ownerID == nil || model.hostID == nil).padding(.horizontal)
            Button("New room", systemImage: "person.3.sequence.fill") { showsNewRoom = true }
                .disabled(!model.paired || model.agents.isEmpty).padding(.horizontal)
            Button("Register this Mac", systemImage: "plus") { Task { await model.register() } }
                .disabled(!model.paired || model.registering).padding(.horizontal)
            Button("Refresh agents", systemImage: "arrow.clockwise") { Task { await model.refreshAgents() } }
                .disabled(!model.paired).padding(.horizontal)
            Button("Handoffs", systemImage: "arrow.triangle.branch") { showsHandoffs = true }
                .disabled(!model.paired).padding(.horizontal)
        }.padding(.vertical)
    }

    private var conversation: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text(model.roomTitle ?? selected?.name ?? "Bots").font(.title2).lineLimit(1)
                    Text(model.storageLabel).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Reconnect", systemImage: "arrow.triangle.2.circlepath") { Task { await model.reconnect() } }
                    .labelStyle(.iconOnly).help("Reconnect")
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
                    Text(BotsModel.canMessageAgent(selected) ? "Replies run on this agent’s computer. Keep Loki’s Den open there; messages wait here while it is unavailable." : "This agent is not configured for chat here yet.")
                        .font(.callout).foregroundStyle(.secondary).padding()
                }
                Divider()
                HStack(alignment: .bottom) {
                    BotsComposer(
                        text: Binding(get: { model.draft }, set: { model.draft = $0; model.saveDraft() }),
                        label: model.isRoom ? "Message the room" : "Message your agent",
                        canSend: canSend,
                        send: { Task { await model.send() } }
                    )
                    .frame(height: 60)
                    .overlay(alignment: .topLeading) {
                        if model.draft.isEmpty {
                            Text(model.isRoom ? "Message the room…" : "Message your agent…")
                                .foregroundStyle(.secondary).padding(8).allowsHitTesting(false)
                        }
                    }
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(.secondary.opacity(0.3)))
                    Button(model.sending ? "Sending…" : "Send", systemImage: "arrow.up") { Task { await model.send() } }
                        .buttonStyle(.borderedProminent).disabled(!canSend)
                        .keyboardShortcut(.return, modifiers: .command)
                }.padding()
            }
        }.frame(minWidth: 0, maxWidth: .infinity, maxHeight: .infinity)
    }

    private var messages: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    if model.messages.isEmpty && !model.loading {
                        Text("Say hello to start your conversation.").foregroundStyle(.secondary).padding()
                    }
                    ForEach(model.messages, id: \.id) { message in
                        BotsMessageRow(message: message, agentName: model.authorName(message), deliveryNote: model.deliveryNotes[message.id])
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
    let deliveryNote: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(agentName).font(.headline)
                if let date = ISO8601DateFormatter().date(from: message.createdAt) {
                    Text(date, style: .time).font(.caption).foregroundStyle(.secondary)
                }
            }
            Text(message.body ?? "Message without text").textSelection(.enabled)
            if message.authorKind == "user", let deliveryNote {
                Text(deliveryNote).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(message.authorKind == "user" ? Color.accentColor.opacity(0.08) : Color.secondary.opacity(0.07), in: .rect(cornerRadius: 12))
        .accessibilityElement(children: .combine)
    }
}
