import SwiftUI

/// Explicit human-triggered exports; connected accounts are never exposed as model tools.
struct GoogleTextActions: View {
    @EnvironmentObject private var google: GoogleAuthManager
    let text: String
    let filename: String
    var mimeType = "text/plain"
    var contentName = "Transcript"
    @State private var busy = false
    @State private var note: String?
    @State private var email: EmailDraft?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if google.isConnected {
                HStack {
                    Button("Save to Drive") {
                        let content = text
                        let name = filename
                        busy = true
                        note = nil
                        Task {
                            defer { busy = false }
                            do {
                                _ = try await google.createDriveFile(name: name, mimeType: mimeType, content: content)
                                note = "Saved to Drive."
                            } catch {
                                note = (error as? GoogleConnectorError)?.description ?? error.localizedDescription
                            }
                        }
                    }
                    Button("Email \(contentName)") { email = EmailDraft(subject: filename, body: text, contentName: contentName) }
                }
                .disabled(busy || text.isEmpty)
                if busy { ProgressView("Saving…").controlSize(.small) }
            } else {
                SettingsNote("Connect Google in Settings to save or email \(contentName.lowercased()) exports.")
            }
            if let note { SettingsNote(note) }
        }
        .sheet(item: $email) { draft in
            GoogleEmailSheet(draft: draft)
                .environmentObject(google)
        }
    }
}

private struct EmailDraft: Identifiable {
    let id = UUID()
    let subject: String
    let body: String
    let contentName: String
}

private struct GoogleEmailSheet: View {
    @EnvironmentObject private var google: GoogleAuthManager
    @Environment(\.dismiss) private var dismiss
    let draft: EmailDraft
    @State private var recipient = ""
    @State private var subject: String
    @State private var busy = false
    @State private var sent = false
    @State private var error: String?

    init(draft: EmailDraft) {
        self.draft = draft
        // An editable copy of this immutable sheet's captured draft.
        _subject = State(initialValue: draft.subject)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Email \(draft.contentName)").font(.headline)
            TextField("Recipient email address", text: $recipient)
            TextField("Subject", text: $subject)
            ScrollView { Text(draft.body).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                .frame(height: 180)
            Text("Sends the content shown above from your connected Google account.").font(.caption)
            if let error { SettingsNote(error) }
            if sent { SettingsNote("Email sent.") }
            HStack {
                Button(sent ? "Done" : "Cancel") { dismiss() }.disabled(busy)
                Spacer()
                if !sent {
                    Button(busy ? "Sending…" : "Send Email") {
                        busy = true
                        error = nil
                        Task {
                            defer { busy = false }
                            do {
                                try await google.sendGmail(to: recipient, subject: subject, body: draft.body)
                                sent = true
                            } catch {
                                self.error = ((error as? GoogleConnectorError)?.description ?? error.localizedDescription)
                                    + " If the connection failed, check Sent Mail before retrying to avoid a duplicate."
                            }
                        }
                    }
                    .disabled(busy || recipient.isEmpty || !google.isConnected)
                }
            }
        }
        .textFieldStyle(.roundedBorder)
        .padding(20)
        .frame(width: 480)
        .interactiveDismissDisabled(busy)
    }
}
