import SwiftUI
import OHHiveFFI

/// Submit a feature request from wherever you're working -- the richer list/voting experience
/// (see `hive.feature_request_list`/`_vote`) is a website feature (apps/web/app/requests), since
/// it's a bigger, list-shaped UI that fits a browser better; this is just "send one in" (Jack,
/// 2026-09-12). Goes through `HiveNode.submitFeatureRequest`, which resolves this node's key to
/// its owning member server-side -- the desktop app never holds a member session to submit as.
struct FeedbackView: View {
    @EnvironmentObject private var store: HiveStore
    @State private var title = ""
    @State private var description = ""
    @State private var busy = false
    @State private var error: String?
    @State private var sent = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Tell us what you'd like Hive to do next. Everything suggested here (and everyone's votes on it) shows up at hive's Feature requests page on the web.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.secondary.opacity(0.08))
                    .clipShape(RoundedRectangle(cornerRadius: 6))
            }
            if sent {
                Text("Thanks -- your suggestion was submitted.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.green.opacity(0.12))
                    .clipShape(RoundedRectangle(cornerRadius: 6))
            }

            Text("One line: what should Hive do?").font(.caption).foregroundStyle(.secondary)
            TextField("", text: $title)
                .textFieldStyle(.roundedBorder)
                .disabled(busy)

            Text("Anything that'd help us understand it (optional)").font(.caption).foregroundStyle(.secondary)
            TextEditor(text: $description)
                .frame(minHeight: 100, maxHeight: 180)
                .font(.callout)
                .disabled(busy)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.3)))

            HStack {
                Button("Submit") { Task { await submit() } }
                    .disabled(busy || title.trimmingCharacters(in: .whitespacesAndNewlines).count < 3)
                if busy { ProgressView().controlSize(.small) }
            }
        }
        .padding()
        .navigationTitle("Feedback")
    }

    private func submit() async {
        error = nil
        sent = false
        busy = true
        defer { busy = false }
        do {
            try await store.submitFeatureRequest(title: title.trimmingCharacters(in: .whitespacesAndNewlines), description: description.trimmingCharacters(in: .whitespacesAndNewlines))
            sent = true
            title = ""
            description = ""
        } catch {
            self.error = "Couldn't submit that: \(error.localizedDescription)"
        }
    }
}
