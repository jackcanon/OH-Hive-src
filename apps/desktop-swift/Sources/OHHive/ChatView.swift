import SwiftUI

/// UI for the ADR-015 local chat engine (ADR-018 decision 5/amendment decision 7). BYOK
/// (`ChatProvider.byok`) has been the default provider since 2026-09-13 -- a general-purpose
/// assistant routed through the member's own configured API key, same mechanism as every other
/// Hive network call (see `ChatEngine.swift`'s header doc). The opt-in on-device provider stays
/// narrower: a small utility panel for asking about this machine's own Hive status specifically
/// (see `NodeStatusTool` in `ChatEngine.swift`) -- the removed placeholder text below used to
/// describe that on-device-only framing and was stale once BYOK became the default (2026-09-15,
/// Jack: "no longer relevant").
struct ChatView: View {
    @EnvironmentObject private var store: HiveStore
    @EnvironmentObject private var chatSessions: ChatSessionStore
    /// Which saved session this instance shows -- set by `ContentView`'s sidebar selection.
    /// Always a real `ChatSession.id`: "+ New chat" creates one immediately (see
    /// `ChatSessionStore.createSession()`'s doc) rather than this view ever representing an
    /// unsaved draft chat.
    let sessionId: UUID

    // `ChatEngine` needs the real `HiveStore`/`ChatSessionStore` from the environment, which isn't
    // available until the view has a body -- so this is built lazily on first appearance (`.task`
    // below) rather than via `@StateObject`'s eager init, which would run before they're in scope.
    @StateObject private var holder = ChatEngineHolder()
    @State private var draft = ""
    // 2026-09-13, Jack: "hide it for now behind a setting for end users to turn on if they
    // want to" -- same UserDefaults key as SettingsView's "On-device chat (experimental)"
    // toggle, so flipping it there updates the picker here live. Off by default.
    @AppStorage("hive.chat.showOnDeviceOption") private var showOnDeviceOption = false

    private var engine: ChatEngine { holder.engine! }
    private var visibleProviders: [ChatProvider] {
        ChatProvider.allCases.filter { $0 != .systemOnDevice || showOnDeviceOption }
    }

    var body: some View {
        Group {
            if holder.engine != nil {
                content
            } else {
                Color.clear
            }
        }
        .navigationTitle(holder.engine?.sessionTitle ?? "Chat")
        .toolbar {
            ToolbarItem {
                if let engine = holder.engine { ChatGoogleExport(engine: engine) }
            }
        }
        // One task, keyed on sessionId, so "create the engine if needed" and "open this session"
        // always happen in order -- two separate `.task`s here would race on which runs first.
        .task(id: sessionId) {
            if holder.engine == nil { holder.engine = ChatEngine(store: store, sessions: chatSessions) }
            holder.engine?.open(sessionId)
        }
    }

    private var content: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        ForEach(engine.messages) { message in
                            bubble(message).id(message.id)
                        }
                        if engine.isResponding {
                            HStack { ProgressView().controlSize(.small); Text("Thinking\u{2026}").font(.caption).foregroundStyle(.secondary) }
                                .padding(.horizontal)
                        }
                    }
                    .padding(.vertical, 12)
                }
                .onChange(of: engine.messages.count) { _, _ in
                    if let last = engine.messages.last {
                        withAnimation { proxy.scrollTo(last.id, anchor: .bottom) }
                    }
                }
            }

            Divider()

            // The composer: provider/model picker directly above the input box, not up at the
            // top of the view -- 2026-09-13, Jack: "The provider and model picker is not in the
            // expected location. I expected it to be nested by the chat box," matching Cowork/
            // ChatGPT-style layouts where the model selector lives right next to where you type,
            // not as a separate bar above the whole conversation.
            VStack(alignment: .leading, spacing: 6) {
                // On-device vs BYOK -- only worth showing once there's a real second option to
                // pick between (Settings' "On-device chat (experimental)" toggle). With just one
                // visible provider this still rendered as a segmented control with a single,
                // permanently-selected segment reading "Your API key" -- inert chrome, nothing
                // to actually pick (2026-09-15, Jack: "the Provider tab from back in the old days
                // of having the Apple Intelligence box... doesn't feel like a necessary field
                // without the context of using Apple Intelligence").
                if visibleProviders.count > 1 {
                    Picker("Provider", selection: Binding(get: { engine.provider }, set: { engine.provider = $0 })) {
                        ForEach(visibleProviders) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 260)
                }

                if engine.provider == .byok {
                    HStack {
                        if engine.byokKeysStatus == nil {
                            Text("Loading your keys\u{2026}")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        } else if engine.byokProvider == nil {
                            Text("No API key on file \u{2014} add one in Settings on the web app.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        } else {
                            // The two-stage picker Jack asked for: provider first, model second --
                            // same pattern as Cowork/Codex/Hermes. `liveModels` + the `.task`
                            // below (2026-09-15) are what actually populate stage 2 now -- before
                            // this it was Default/Custom only, every time ("not actually loading
                            // with models").
                            ProviderModelPicker(
                                keysStatus: engine.byokKeysStatus,
                                provider: Binding(get: { engine.byokProvider }, set: { engine.byokProvider = $0 }),
                                model: Binding(get: { engine.byokModel }, set: { engine.byokModel = $0 }),
                                liveModels: engine.byokProvider.flatMap { engine.byokModelsByProvider[$0] }
                            )
                            .task(id: engine.byokProvider) {
                                if let p = engine.byokProvider { await engine.loadByokModelsIfNeeded(provider: p) }
                            }
                            Spacer()
                            Text("Billed to your own account \u{2014} nothing charged to Honey.")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                } else if let note = engine.availabilityNote {
                    Text(note)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .task(id: engine.provider) {
                if engine.provider == .byok { await engine.loadByokKeysIfNeeded() }
            }
            // Guards a stale/legacy session (or the toggle being switched off mid-session) that
            // left `engine.provider == .systemOnDevice` while it's hidden -- snap back to BYOK
            // rather than leaving the composer showing nothing actionable.
            .onChange(of: showOnDeviceOption) { _, stillShown in
                if !stillShown && engine.provider == .systemOnDevice { engine.provider = .byok }
            }
            .onAppear {
                if !showOnDeviceOption && engine.provider == .systemOnDevice { engine.provider = .byok }
            }
            .padding(.horizontal, 10)
            .padding(.top, 8)

            HStack {
                TextField("Ask something\u{2026}", text: $draft, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...4)
                    .chatSubmit(enabled: !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !engine.isResponding, action: send)
                Button("Send", action: send)
                    .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || engine.isResponding)
            }
            .padding(10)
        }
    }

    @ViewBuilder private func bubble(_ message: ChatMessage) -> some View {
        HStack {
            if message.role == .user { Spacer(minLength: 40) }
            Text(message.text)
                .font(.callout)
                .padding(10)
                .background(background(for: message.role))
                .foregroundStyle(message.role == .user ? .white : .primary)
                .clipShape(RoundedRectangle(cornerRadius: 10))
            if message.role != .user { Spacer(minLength: 40) }
        }
        .padding(.horizontal)
    }

    private func background(for role: ChatMessage.Role) -> Color {
        switch role {
        case .user: return Color.accentColor
        case .assistant: return Color.secondary.opacity(0.15)
        case .system: return Color.orange.opacity(0.15)
        }
    }

    private func send() {
        guard !engine.isResponding, !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        let text = draft
        draft = ""
        Task { await engine.send(text) }
    }
}
