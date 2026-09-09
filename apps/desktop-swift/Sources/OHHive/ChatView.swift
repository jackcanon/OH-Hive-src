import SwiftUI

/// First-slice UI for the ADR-015 local chat engine (ADR-018 decision 5/amendment decision 7).
/// Deliberately plain -- a small utility panel for asking the on-device model about this
/// machine's own Hive status, not a general-purpose chat product.
struct ChatView: View {
    @EnvironmentObject private var store: HiveStore
    // `ChatEngine` needs the real `HiveStore` from the environment, which isn't available until
    // the view has a body -- so this is built lazily on first appearance (`.task` below) rather
    // than via `@StateObject`'s eager init, which would run before `store` is in scope.
    @StateObject private var holder = ChatEngineHolder()
    @State private var draft = ""

    private var engine: ChatEngine { holder.engine! }

    var body: some View {
        Group {
            if holder.engine != nil {
                content
            } else {
                Color.clear
            }
        }
        .navigationTitle("Chat")
        .task {
            if holder.engine == nil { holder.engine = ChatEngine(store: store) }
        }
    }

    private var content: some View {
        VStack(spacing: 0) {
            if let note = engine.availabilityNote {
                Text(note)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.secondary.opacity(0.08))
            }
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        if engine.messages.isEmpty {
                            Text("Ask about this machine's Hive status \u{2014} \u{201c}am I paired?\u{201d}, \u{201c}what models do I have?\u{201d}, \u{201c}is my server running?\u{201d}")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .padding()
                        }
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
            HStack {
                TextField("Ask something\u{2026}", text: $draft, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...4)
                    .onSubmit(send)
                Button("Send", action: send)
                    .disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty || engine.isResponding)
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
        let text = draft
        draft = ""
        Task { await engine.send(text) }
    }
}
