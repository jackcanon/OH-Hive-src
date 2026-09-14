import Foundation
import FoundationModels

/// A pre-1.0-only, everywhere-available assistant for turning a casual description into a
/// feature request or bug report and submitting it -- 2026-09-13, Jack: "it should be a separate
/// swift only app feature and it should be on every screen of the swift app so that you can call
/// it from everywhere. we'll keep it everpresent until we hit a 1.0 release."
///
/// Deliberately NOT built on `ChatEngine`/`ChatProvider` even though the shape rhymes with
/// `ChatEngine.sendOnDevice` -- Jack asked for this as its own separate feature, not another mode
/// of the general chat panel, so it isn't affected by the on-device chat opt-in toggle
/// (`hive.chat.showOnDeviceOption`) at all, has its own tiny `LanguageModelSession` scoped only to
/// feedback, and can be deleted as one clean unit (this file + `FeedbackAssistantView.swift`) once
/// `HiveVersion.isPre1_0` retires it for good at 1.0. Same on-device guardrail as `ChatEngine`
/// applies here too (ADR-018 decision 11): this only ever calls this node's own
/// `submitFeatureRequest`/`submitBugReport`, never anything touching another member's paid work.
@MainActor
final class FeedbackAssistant: ObservableObject {
    @Published var messages: [ChatMessage] = []
    @Published var isResponding = false
    @Published var availabilityNote: String?

    private var session: LanguageModelSession?
    private let store: HiveStore

    init(store: HiveStore) {
        self.store = store
        checkAvailability()
    }

    private func checkAvailability() {
        switch SystemLanguageModel.default.availability {
        case .available:
            availabilityNote = nil
            session = LanguageModelSession(
                tools: [SubmitFeatureRequestTool(store: store), SubmitBugReportTool(store: store)],
                instructions: """
                You help a Hive member turn a casual, spoken-style description into a well-formed \
                feature request or bug report and submit it right away. If the description is too \
                vague to act on (a bug with no idea what was expected/what happened, or a feature \
                idea with no clear one-line summary), ask at most one short clarifying question --
                don't interrogate them. As soon as you have enough, submit with the matching tool \
                immediately -- don't ask "should I submit this?" first, that's an extra round trip \
                for something they already came here to do. After submitting, confirm in one short \
                line what was sent and that it's now visible on Hive's website. Never submit the \
                same thing twice. Default bug reports to not-anonymous unless they say otherwise. \
                If they ask something unrelated to submitting feedback, say plainly that this \
                assistant only handles feature requests and bug reports, and point them to the \
                Feedback section in Settings for a plain form, or the regular chat for anything else.
                """
            )
        case .unavailable(let reason):
            session = nil
            availabilityNote = Self.describe(reason)
        @unknown default:
            session = nil
            availabilityNote = "Apple Intelligence isn't available on this machine right now."
        }
    }

    private static func describe(_ reason: SystemLanguageModel.Availability.UnavailableReason) -> String {
        switch reason {
        case .deviceNotEligible:
            return "This Mac isn't eligible for Apple Intelligence, so this assistant can't run here -- use the Feedback section in Settings instead."
        case .appleIntelligenceNotEnabled:
            return "Turn on Apple Intelligence in System Settings to use this assistant, or use the Feedback section in Settings instead."
        case .modelNotReady:
            return "The on-device model is still downloading -- try again shortly, or use the Feedback section in Settings instead."
        @unknown default:
            return "This assistant isn't available right now -- use the Feedback section in Settings instead."
        }
    }

    func send(_ text: String) async {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        checkAvailability()
        guard let session else {
            messages.append(ChatMessage(role: .system, text: availabilityNote ?? "Assistant unavailable."))
            return
        }
        messages.append(ChatMessage(role: .user, text: trimmed))
        isResponding = true
        defer { isResponding = false }
        do {
            let response = try await session.respond(to: trimmed)
            messages.append(ChatMessage(role: .assistant, text: response.content))
        } catch {
            messages.append(ChatMessage(role: .system, text: "Couldn't get a response: \(error.localizedDescription)"))
        }
    }
}

/// Submits a feature request the member described in plain language -- same underlying RPC as
/// `FeedbackView`'s `FeatureRequestForm.submit()` (`HiveNode.submitFeatureRequest`), just reached
/// via tool-calling instead of a form.
struct SubmitFeatureRequestTool: Tool {
    let name = "submitFeatureRequest"
    let description = "Submits a feature request to Hive's public feature-request board."

    @Generable
    struct Arguments {
        @Guide(description: "A short, one-line summary of the requested feature.")
        let title: String
        @Guide(description: "Supporting detail: why it'd help, how it should work. Empty string if there's nothing more to add.")
        let description: String
    }

    let store: HiveStore

    func call(arguments: Arguments) async throws -> String {
        do {
            try await store.submitFeatureRequest(title: arguments.title, description: arguments.description)
            return "Submitted to Hive's Feature requests page."
        } catch {
            return "Couldn't submit that: \(error.localizedDescription)"
        }
    }
}

/// Submits a bug report the member described in plain language -- same underlying RPC as
/// `FeedbackView`'s `BugReportForm.submit()` (`HiveNode.submitBugReport`), just reached via
/// tool-calling instead of a form.
struct SubmitBugReportTool: Tool {
    let name = "submitBugReport"
    let description = "Submits a bug report to Hive's public bug-tracker board."

    @Generable
    struct Arguments {
        @Guide(description: "A short, one-line summary of what's broken.")
        let title: String
        @Guide(description: "What was expected, what happened instead, and how to reproduce it. Empty string if there's nothing more to add.")
        let description: String
        @Guide(description: "True only if the member explicitly asked to submit anonymously (their name hidden from everyone, including admins). Default false.")
        let anonymous: Bool
    }

    let store: HiveStore

    func call(arguments: Arguments) async throws -> String {
        do {
            try await store.submitBugReport(title: arguments.title, description: arguments.description, anonymous: arguments.anonymous)
            return "Submitted to Hive's Bug reports page."
        } catch {
            return "Couldn't submit that: \(error.localizedDescription)"
        }
    }
}

@MainActor
final class FeedbackAssistantHolder: ObservableObject {
    @Published var assistant: FeedbackAssistant?
}

/// Version gate for this whole feature -- 2026-09-13, Jack: "we'll keep it everpresent until we
/// hit a 1.0 release." Reads `store.about.appVersion` (the `ohhive-ffi` crate's `CARGO_PKG_VERSION`,
/// same string `UpdateChecker`/`AboutView` already use) so crossing the workspace version to
/// 1.0.0 makes the floating button disappear on its own, with no separate cleanup step to
/// remember -- and, just as important, nobody can ship 1.0.0 without *noticing* this vanished,
/// which is exactly the kind of forcing function Jack's separate versioning note asked for
/// ("we may need to do more incremental update numbers so we don't get to a 1.0 before its really
/// ready") -- see this file's presence being tied to the major version as a small, free assist
/// toward not backing into 1.0.0 by accident.
enum HiveVersion {
    /// Unparseable input fails open (treated as pre-1.0) -- better to keep a temporary feature
    /// visible on a version-string surprise than to silently hide it.
    static func isPre1_0(_ version: String) -> Bool {
        guard let majorText = version.split(separator: ".").first, let major = Int(majorText) else {
            return true
        }
        return major < 1
    }
}
