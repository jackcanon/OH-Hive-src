import SwiftUI

struct ChatExportSnapshot: Identifiable {
    let id = UUID()
    let filename: String
    let markdown: String

    init(title: String, messages: [ChatMessage]) {
        let cleanTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = cleanTitle.isEmpty ? "Chat" : cleanTitle
        filename = name + ".md"
        markdown = messages.map { message in
            let role: String
            switch message.role {
            case .user: role = "User"
            case .assistant: role = "Assistant"
            case .system: role = "System"
            }
            return "## \(role)\n\n\(message.text)"
        }.joined(separator: "\n\n---\n\n") + "\n"
    }
}

struct ChatGoogleExport: View {
    @EnvironmentObject private var google: GoogleAuthManager
    @ObservedObject var engine: ChatEngine
    @State private var snapshot: ChatExportSnapshot?

    var body: some View {
        if google.isConnected && !engine.messages.isEmpty {
            Button("Export Chat", systemImage: "square.and.arrow.up") {
                snapshot = ChatExportSnapshot(title: engine.sessionTitle, messages: engine.messages)
            }
            .disabled(engine.isResponding)
            .help("Save this chat to Google Drive or email a copy")
            .sheet(item: $snapshot) { export in
                ChatExportSheet(export: export)
                    .environmentObject(google)
            }
        }
    }
}

private struct ChatExportSheet: View {
    @Environment(\.dismiss) private var dismiss
    let export: ChatExportSnapshot
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(export.filename).font(.headline)
            ScrollView { Text(export.markdown).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                .frame(height: 240)
            GoogleTextActions(text: export.markdown, filename: export.filename, mimeType: "text/markdown", contentName: "Chat")
            Button("Done") { dismiss() }
        }
        .padding(20)
        .frame(width: 540)
    }
}
