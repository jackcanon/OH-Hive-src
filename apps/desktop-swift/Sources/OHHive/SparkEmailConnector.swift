import Foundation
import Combine

/// Narrow command builders: never accept arbitrary CLI flags or actions from email content.
enum SparkEmailCommands {
    static func id(_ value: String) throws -> String {
        guard !value.isEmpty, value.count <= 20, value.allSatisfy({ $0.isASCII && $0.isNumber }) else {
            throw SparkImportError(message: "Enter the numeric message ID shown in Spark’s results.")
        }
        return value
    }
    static func draft(account: String, to: String, subject: String, body: String) throws -> [String] {
        for address in [account, to] {
            guard address.contains("@"), !address.contains(where: { $0.isWhitespace }),
                  !address.contains(","), !address.hasPrefix("-"), address.count <= 254 else {
                throw SparkImportError(message: "Enter one email address for From and To.")
            }
        }
        guard !subject.isEmpty, !subject.contains("\n"), !subject.contains("\r"), subject.count <= 500,
              !body.isEmpty, body.utf8.count <= 32_000 else {
            throw SparkImportError(message: "Add a subject and message (up to 32 KB).")
        }
        return ["draft", "--account", account, "--to", to, "--subject", subject, "--body", body]
    }
    static func organize(_ action: String, message: String) throws -> [String] {
        guard ["archive", "moveToInbox", "pin", "unpin", "markAsSeen", "markAsUnseen"].contains(action) else {
            throw SparkImportError(message: "That email action is not available.")
        }
        return ["action", action, try id(message)]
    }
}

@MainActor
final class SparkEmailConnector: ObservableObject {
    @Published var readEnabled: Bool { didSet { defaults.set(readEnabled, forKey: "spark.email.read") } }
    @Published var draftEnabled: Bool { didSet { defaults.set(draftEnabled, forKey: "spark.email.draft") } }
    @Published var organizeEnabled: Bool { didSet { defaults.set(organizeEnabled, forKey: "spark.email.organize") } }
    @Published var sendEnabled: Bool { didSet { defaults.set(sendEnabled, forKey: "spark.email.send") } }
    @Published private(set) var busy = false
    @Published private(set) var result = ""
    @Published private(set) var status = ""
    @Published private(set) var emailRows: [(id: String, label: String)] = []
    @Published private(set) var threadID: String?
    @Published private(set) var threadText = ""
    private let defaults: UserDefaults
    private let command: ([String]) async throws -> String
    init(defaults: UserDefaults = .standard, command: @escaping ([String]) async throws -> String = SparkMeetingCLI.run) {
        self.command = command
        self.defaults = defaults
        readEnabled = defaults.bool(forKey: "spark.email.read")
        draftEnabled = defaults.bool(forKey: "spark.email.draft")
        organizeEnabled = defaults.bool(forKey: "spark.email.organize")
        sendEnabled = defaults.bool(forKey: "spark.email.send")
    }
    private func run(_ args: [String], allowed: Bool, mutation: Bool = false) async throws -> String {
        guard allowed else { throw SparkImportError(message: "Enable this email capability first.") }
        guard !busy else { throw SparkImportError(message: "Wait for the current Spark operation to finish.") }
        busy = true
        defer { busy = false }
        do {
            let value = try await command(args)
            status = mutation ? "Spark processed the request. Review its result below." : "Loaded from Spark."
            return value
        } catch {
            status = mutation ? "Spark did not confirm the result. Check Spark before retrying to avoid duplicate actions." : error.localizedDescription
            throw error
        }
    }
    func search(filter: String, page: Int) async {
        do {
            guard filter.count <= 1000, (1...100).contains(page) else { return }
            result = try await run(["emails", "--filter", filter, "--page", String(page), "--page-size", "20"], allowed: readEnabled)
            emailRows = result.split(separator: "\n").compactMap { line in
                let parts = line.split(whereSeparator: { $0.isWhitespace })
                guard let first = parts.first, (try? SparkEmailCommands.id(String(first))) != nil else { return nil }
                return (String(first), String(line.trimmingCharacters(in: .whitespaces)))
            }
        } catch { status = error.localizedDescription }
    }
    func read(_ message: String) async {
        threadID = nil; threadText = ""
        do {
            let id = try SparkEmailCommands.id(message)
            let text = try await run(["thread", id], allowed: readEnabled)
            threadID = id; threadText = text; result = text
        } catch { status = error.localizedDescription }
    }
    func createDraft(account: String, to: String, subject: String, body: String) async {
        do { result = try await run(SparkEmailCommands.draft(account: account, to: to, subject: subject, body: body), allowed: draftEnabled, mutation: true) }
        catch { if !status.contains("did not confirm") { status = error.localizedDescription } }
    }
    func organize(_ action: String, message: String) async {
        do { result = try await run(SparkEmailCommands.organize(action, message: message), allowed: organizeEnabled, mutation: true) }
        catch { if !status.contains("did not confirm") { status = error.localizedDescription } }
    }
    func sendReviewedDraft(_ message: String, expected: String) async {
        do {
            let current = try await run(["thread", SparkEmailCommands.id(message)], allowed: readEnabled && sendEnabled)
            guard current == expected else {
                status = "The draft changed. Read it again before sending."
                return
            }
            result = try await run(["action", "send", SparkEmailCommands.id(message)], allowed: sendEnabled, mutation: true) }
        catch { if !status.contains("did not confirm") { status = error.localizedDescription } }
    }
}
