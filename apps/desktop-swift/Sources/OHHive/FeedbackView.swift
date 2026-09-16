import SwiftUI
import OHHiveFFI

/// Submit a feature request or bug report from wherever you're working -- the richer list/voting/
/// comments/attachments experience (see `hive.feature_request_list`/`_vote`,
/// `hive.bug_report_list`/`_comment_*`/`_follow`) is a website feature (apps/web/app/requests),
/// since it's a bigger, list-shaped UI that fits a browser better; this is just "send one in"
/// (Jack, 2026-09-12 for feature requests, 2026-09-13 for bug reports). Goes through
/// `HiveNode.submitFeatureRequest`/`submitBugReport`, which resolve this node's key to its owning
/// member server-side -- the desktop app never holds a member session to submit as.
struct FeedbackView: View {
    private enum Kind: String, CaseIterable, Identifiable {
        case feature = "Feature request", bug = "Bug report"
        var id: String { rawValue }
    }
    @State private var kind: Kind = .feature

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Kind", selection: $kind) {
                ForEach(Kind.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)

            switch kind {
            case .feature: FeatureRequestForm()
            case .bug: BugReportForm()
            }
        }
        .padding()
        .navigationTitle("Feedback")
    }
}

private struct FeatureRequestForm: View {
    @EnvironmentObject private var store: HiveStore
    @State private var title = ""
    @State private var description = ""
    @State private var busy = false
    @State private var error: String?
    @State private var sent = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Tell us what you'd like Hive to do next. Everything suggested here (and everyone's votes on it) shows up on Hive's Feature requests page on the web.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let error { SettingsNote(error) }
            if sent { SettingsNote("Thanks -- your suggestion was submitted.", ok: true) }

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

private struct BugReportForm: View {
    @EnvironmentObject private var store: HiveStore
    @State private var title = ""
    @State private var description = ""
    @State private var anonymous = false
    @State private var busy = false
    @State private var error: String?
    @State private var sent = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Something not working as it should? Report it here -- signed or anonymous. It'll show up on Hive's Bug reports page on the web, where you (or anyone) can add screenshots, logs, and follow-up comments.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let error { SettingsNote(error) }
            if sent { SettingsNote("Thanks -- your report was submitted.", ok: true) }

            Text("One line: what's broken?").font(.caption).foregroundStyle(.secondary)
            TextField("", text: $title)
                .textFieldStyle(.roundedBorder)
                .disabled(busy)

            Text("What did you expect, what happened instead, and how to reproduce it (optional)").font(.caption).foregroundStyle(.secondary)
            TextEditor(text: $description)
                .frame(minHeight: 100, maxHeight: 180)
                .font(.callout)
                .disabled(busy)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.3)))

            Toggle("Submit anonymously -- your name won't be shown to anyone, including admins", isOn: $anonymous)
                .toggleStyle(.checkbox)
                .disabled(busy)
                .font(.caption)

            HStack {
                Button("Submit") { Task { await submit() } }
                    .disabled(busy || title.trimmingCharacters(in: .whitespacesAndNewlines).count < 3)
                if busy { ProgressView().controlSize(.small) }
            }
        }
    }

    private func submit() async {
        error = nil
        sent = false
        busy = true
        defer { busy = false }
        do {
            try await store.submitBugReport(
                title: title.trimmingCharacters(in: .whitespacesAndNewlines),
                description: description.trimmingCharacters(in: .whitespacesAndNewlines),
                anonymous: anonymous
            )
            sent = true
            title = ""
            description = ""
            anonymous = false
        } catch {
            self.error = "Couldn't submit that: \(error.localizedDescription)"
        }
    }
}
