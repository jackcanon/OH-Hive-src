import SwiftUI
import OHHiveFFI

struct SparkEmailSettings: View {
    @EnvironmentObject private var store: HiveStore
    @ObservedObject var connector: SparkEmailConnector
    @State private var expanded = false
    @State private var filter = "is:unread"
    @State private var page = 1
    @State private var messageID = ""
    @State private var account = ""
    @State private var recipient = ""
    @State private var subject = ""
    @State private var bodyText = ""
    @State private var action = "archive"
    @State private var vaults: [VaultInfo] = []
    @State private var vaultID = ""
    @State private var note = ""
    @State private var reviewedText = ""
    @State private var sendReview: String?
    @State private var reviewPresented = false
    @State private var saving = false

    var body: some View {
        DisclosureGroup("Email", isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 10) {
                Text("Uses accounts enabled in Spark’s AI Agents settings. Drafting and organizing require Spark Triage access; sending requires Send access. These controls operate from this Mac.")
                    .font(.caption).foregroundStyle(.secondary)
                Toggle("Read and search email", isOn: $connector.readEnabled)
                Toggle("Create drafts", isOn: $connector.draftEnabled)
                Toggle("Organize email", isOn: $connector.organizeEnabled)
                Toggle("Send reviewed drafts", isOn: $connector.sendEnabled)
                if connector.readEnabled {
                    HStack {
                        TextField("Search, e.g. from:alex@example.com", text: $filter)
                        Button("Search inbox") { page = 1; Task { await connector.search(filter: filter, page: page) } }
                    }
                    HStack {
                        Button("Previous") { page -= 1; Task { await connector.search(filter: filter, page: page) } }.disabled(page <= 1)
                        Text("Page \(page)")
                        Button("Next") { page += 1; Task { await connector.search(filter: filter, page: page) } }.disabled(page >= 100)
                    }
                    if !connector.emailRows.isEmpty {
                        Picker("Conversation", selection: $messageID) {
                            Text("Choose a message").tag("")
                            ForEach(connector.emailRows, id: \.id) { row in Text(row.label).tag(row.id) }
                        }
                    }
                    DisclosureGroup("Enter a message ID manually") {
                        TextField("Message ID from Spark", text: $messageID)
                    }
                    Button("Read conversation") { Task { await connector.read(messageID) } }
                    Picker("Save conversation to Vault", selection: $vaultID) {
                        Text("Choose a Vault").tag("")
                        ForEach(vaults, id: \.id) { Text($0.name).tag($0.id) }
                    }
                    Button("Save loaded conversation") { Task { await saveThread() } }
                        .disabled(vaultID.isEmpty || connector.threadID == nil || saving)
                }
                if connector.draftEnabled {
                    DisclosureGroup("Compose a draft") {
                        TextField("From email address", text: $account)
                        TextField("To email address", text: $recipient)
                        TextField("Subject", text: $subject)
                        TextEditor(text: $bodyText).frame(height: 120)
                        Button("Create draft in Spark") { Task { await connector.createDraft(account: account, to: recipient, subject: subject, body: bodyText) } }
                        Text("Creates a draft only. Review it in Spark before sending.").font(.caption)
                    }
                }
                if connector.organizeEnabled {
                    TextField("Message ID to organize", text: $messageID)
                    HStack {
                        Picker("Action", selection: $action) {
                            Text("Archive").tag("archive"); Text("Move to inbox").tag("moveToInbox")
                            Text("Pin").tag("pin"); Text("Unpin").tag("unpin")
                            Text("Mark read").tag("markAsSeen"); Text("Mark unread").tag("markAsUnseen")
                        }
                        Button("Apply to message") { Task { await connector.organize(action, message: messageID) } }
                    }
                }
                if connector.sendEnabled {
                    TextField("Draft ID to send", text: $messageID)
                    Button("Review before sending…") {
                        let id = messageID
                        Task {
                            await connector.read(id)
                            if connector.threadID == id && connector.readEnabled { sendReview = id; reviewedText = connector.threadText; reviewPresented = true }
                        }
                    }.disabled(!connector.readEnabled)
                    Text("Read access is required to review. Sending always requires confirmation.").font(.caption)
                }
                Text(connector.status).font(.caption)
                if !note.isEmpty { Text(note).font(.caption) }
                if connector.busy { ProgressView().controlSize(.small) }
                if !connector.result.isEmpty {
                    ScrollView { Text(connector.result).font(.system(.caption, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                        .frame(height: 220)
                }
            }
            .textFieldStyle(.roundedBorder)
            .disabled(connector.busy)
        }
        .onAppear { vaults = store.vaultOpen()?.vaults ?? [] }
        .sheet(isPresented: $reviewPresented) {
            VStack(alignment: .leading, spacing: 12) {
                Text("Send this Spark draft?").font(.headline)
                Text("Check the recipients and message below. Spark sends the current draft; if you edit it elsewhere, cancel and review again.").font(.caption)
                ScrollView { Text(reviewedText).textSelection(.enabled) }
                HStack {
                    Button("Cancel") { reviewPresented = false; sendReview = nil }
                    Spacer()
                    Button("Send now") {
                        guard let id = sendReview else { return }
                        reviewPresented = false; sendReview = nil
                        Task { await connector.sendReviewedDraft(id, expected: reviewedText) }
                    }.disabled(!connector.sendEnabled)
                }
            }.padding().frame(width: 650, height: 500)
        }
    }
    private func saveThread() async {
        guard connector.readEnabled, let id = connector.threadID, !vaultID.isEmpty else { return }
        saving = true; defer { saving = false }
        do {
            let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/ohhive/spark-email-import")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            let file = root.appendingPathComponent("spark-email-\(id).md")
            let text = "# Spark email conversation \(id)\n\nSource: Spark email \(id)\n\n" + connector.threadText
            try Data(text.utf8).write(to: file, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
            _ = try store.vaultIntakeApproveFile(vaultId: vaultID, root: root.path, relativePath: file.lastPathComponent, project: "email")
            note = "Conversation saved to Vault."
        } catch { note = error.localizedDescription }
    }
}
