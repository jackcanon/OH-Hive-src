import SwiftUI

/// The everywhere-available floating button (see `FeedbackAssistant.swift`'s header doc) --
/// `ContentView` applies this as a `.overlay(alignment: .bottomTrailing)` on the whole window, so
/// it floats above whatever detail screen is showing, Setup included, rather than living inside
/// any one view.
struct FeedbackAssistantButton: View {
    @EnvironmentObject private var store: HiveStore
    @StateObject private var holder = FeedbackAssistantHolder()
    @State private var isPresented = false

    var body: some View {
        Button {
            isPresented = true
        } label: {
            Image(systemName: "sparkles")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 34, height: 34)
                .background(Circle().fill(Color.accentColor))
                .shadow(color: .black.opacity(0.25), radius: 3, y: 1)
        }
        .buttonStyle(.plain)
        .help("Feedback assistant \u{2014} describe a bug or feature idea in your own words")
        .popover(isPresented: $isPresented, arrowEdge: .trailing) {
            FeedbackAssistantPanel(holder: holder)
        }
        .task {
            if holder.assistant == nil { holder.assistant = FeedbackAssistant(store: store) }
        }
    }
}

private struct FeedbackAssistantPanel: View {
    @ObservedObject var holder: FeedbackAssistantHolder
    @State private var draft = ""

    private var assistant: FeedbackAssistant { holder.assistant! }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                Label("Feedback assistant", systemImage: "sparkles")
                    .font(.headline)
                Text("Describe a bug or feature idea in your own words \u{2014} it'll turn it into a proper report and send it. On-device and free, separate from the chat panel. Available on every screen until Hive reaches 1.0.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(12)

            Divider()

            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        if assistant.messages.isEmpty {
                            Text("Try: \u{201c}the app crashed when I clicked Start Working twice\u{201d} or \u{201c}I wish I could rename a chat.\u{201d}")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .padding(.horizontal, 12)
                                .padding(.top, 8)
                        }
                        ForEach(assistant.messages) { message in
                            bubble(message).id(message.id)
                        }
                        if assistant.isResponding {
                            HStack {
                                ProgressView().controlSize(.small)
                                Text("Thinking\u{2026}").font(.caption).foregroundStyle(.secondary)
                            }
                            .padding(.horizontal, 12)
                        }
                    }
                    .padding(.vertical, 8)
                }
                .onChange(of: assistant.messages.count) { _, _ in
                    if let last = assistant.messages.last {
                        withAnimation { proxy.scrollTo(last.id, anchor: .bottom) }
                    }
                }
            }
            .frame(height: 220)

            if let note = assistant.availabilityNote, assistant.messages.isEmpty {
                Text(note)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 8)
            }

            Divider()
            HStack {
                TextField("Describe it\u{2026}", text: $draft, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...4)
                    .onSubmit(send)
                Button("Send", action: send)
                    .disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty || assistant.isResponding)
            }
            .padding(10)
        }
        .frame(width: 320)
    }

    @ViewBuilder private func bubble(_ message: ChatMessage) -> some View {
        HStack {
            if message.role == .user { Spacer(minLength: 30) }
            Text(message.text)
                .font(.callout)
                .padding(8)
                .background(background(for: message.role))
                .foregroundStyle(message.role == .user ? .white : .primary)
                .clipShape(RoundedRectangle(cornerRadius: 8))
            if message.role != .user { Spacer(minLength: 30) }
        }
        .padding(.horizontal, 12)
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
        Task { await assistant.send(text) }
    }
}
