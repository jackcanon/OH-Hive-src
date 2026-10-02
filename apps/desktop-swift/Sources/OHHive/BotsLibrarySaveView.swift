import SwiftUI
import OHHiveFFI

struct BotsLibraryReply: Identifiable {
    let message: BotsMessage
    let agentName: String
    var id: String { message.id }
    static func canSave(_ message: BotsMessage) -> Bool {
        message.authorKind == "agent" && message.kind == "text" &&
        !(message.body ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

/// The member chooses the destination for one saved reply; this never grants an agent writes.
struct BotsLibrarySaveView: View {
    let reply: BotsLibraryReply
    let model: BotsModel
    @Environment(\.dismiss) private var dismiss
    @State private var collections: [VaultInfo] = []
    @State private var selectedID = ""
    @State private var title = ""
    @State private var loading = true
    @State private var saving = false
    @State private var saved: VaultDocument?
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Save reply to Library").font(.title2.bold())
                Spacer()
                Button(saved == nil ? "Cancel" : "Done") { dismiss() }
                    .disabled(saving)
            }
            if let saved {
                Label("Saved in \(collections.first { $0.id == saved.vaultId }?.name ?? "your Library")", systemImage: "checkmark.circle.fill")
                Text(saved.title).font(.headline)
                Text(saved.path).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                Text("You can find this note in the Library collection. Saving the same reply there again returns the existing note and keeps any edits.")
            } else {
                Text("Save \(reply.agentName)’s reply as a new note with its conversation and message origin. Links and citations remain unverified; saving is not a research review.")
                    .foregroundStyle(.secondary)
                if loading {
                    ProgressView("Finding available collections…")
                } else if collections.isEmpty {
                    Text("Create a manual collection in Library, or share one with this computer, then reopen this action. Folder indexes cannot receive chat notes.")
                } else {
                    Picker("Collection", selection: $selectedID) {
                        Text("Choose a collection").tag("")
                        ForEach(collections, id: \.id) { Text($0.name).tag($0.id) }
                    }
                    TextField("Note title", text: $title)
                    ScrollView { Text(reply.message.body ?? "").textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                        .frame(maxHeight: 240)
                    Button(saving ? "Saving…" : "Save to Library") { Task { await save() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(saving || selectedID.isEmpty || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || title.utf8.count > 200)
                }
            }
            if let error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
        }
        .padding(24)
        .frame(minWidth: 480, idealWidth: 600)
        .interactiveDismissDisabled(saving)
        .task {
            title = "Reply from " + reply.agentName
            do {
                collections = try await model.libraryCollections()
            } catch { self.error = "Couldn’t load collections: \(error.localizedDescription)" }
            loading = false
        }
    }

    private func save() async {
        guard !saving else { return }
        saving = true
        error = nil
        do {
            saved = try await model.saveReplyToLibrary(messageID: reply.id, vaultID: selectedID, title: title.trimmingCharacters(in: .whitespacesAndNewlines))
        } catch { self.error = "Couldn’t save the reply: \(error.localizedDescription). You can retry; the same reply won’t create a duplicate." }
        saving = false
    }
}
