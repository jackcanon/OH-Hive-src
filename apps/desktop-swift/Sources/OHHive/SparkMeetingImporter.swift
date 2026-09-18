import Foundation
import Combine
import Darwin
import OHHiveFFI

struct SparkImportError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// Spark currently emits a text table, not a documented JSON format. Fail closed if it changes.
enum SparkMeetingFormat {
    static func page(_ text: String) throws -> (ids: [String], pages: Int) {
        let lines = text.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
        if lines.count == 2, lines[0].hasPrefix("Meeting Notes"), lines[1] == "No meetings found." {
            return ([], 1)
        }
        let footer = try NSRegularExpression(pattern: #"Page (\d+) of (\d+) \((\d+) total meetings\)"#)
        let range = NSRange(text.startIndex..., in: text)
        guard let match = footer.firstMatch(in: text, range: range),
              let pagesRange = Range(match.range(at: 2), in: text),
              let pages = Int(text[pagesRange]), pages > 0, pages <= 100 else {
            // Known empty response is verified separately before accepting it.
            throw SparkImportError(message: "Spark’s meeting list could not be read. Check Spark is open and CLI Read access is enabled.")
        }
        let row = try NSRegularExpression(pattern: #"(?m)^[ \t]*(\d+)[ \t]+.+[ \t]+\d{4}-\d{2}-\d{2}[ \t]+\d{2}:\d{2}[ \t]+.*$"#)
        let ids = row.matches(in: text, range: range).compactMap { m in
            Range(m.range(at: 1), in: text).map { String(text[$0]) }
        }
        guard !ids.isEmpty else { throw SparkImportError(message: "No readable meetings were returned by Spark.") }
        return (ids, pages)
    }

    static func markdown(_ text: String, id: String) throws -> String {
        guard text.utf8.count < 950_000, text.hasPrefix("Meeting: "),
              text.contains("\nDate: "), let first = text.split(separator: "\n").first else {
            throw SparkImportError(message: "Spark returned an incomplete or oversized meeting. It was not imported.")
        }
        let title = first.dropFirst("Meeting: ".count)
        return "# \(title)\n\nSource: Spark meeting \(id)\n\n" + text
    }
}

/// Run a fixed executable/argument vector off the UI thread; no shell or model involved.
enum SparkMeetingCLI {
    static func run(_ arguments: [String]) async throws -> String {
        try await Task.detached(priority: .utility) {
            let paths = ["/usr/local/bin/spark", "/opt/homebrew/bin/spark"]
            guard let binary = paths.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
                throw SparkImportError(message: "Open Spark → Settings → AI Agents → Setup CLI, then allow Read access.")
            }
            let fm = FileManager.default
            let dir = fm.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            try fm.createDirectory(at: dir, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            defer { try? fm.removeItem(at: dir) }
            let output = dir.appendingPathComponent("output")
            _ = fm.createFile(atPath: output.path, contents: nil, attributes: [.posixPermissions: 0o600])
            let handle = try FileHandle(forWritingTo: output)
            defer { try? handle.close() }
            let process = Process()
            process.executableURL = URL(fileURLWithPath: binary)
            process.arguments = arguments
            process.standardOutput = handle
            process.standardError = FileHandle.nullDevice
            process.standardInput = FileHandle.nullDevice
            try process.run()
            let deadline = Date().addingTimeInterval(30)
            while process.isRunning {
                let size = (try? fm.attributesOfItem(atPath: output.path)[.size] as? NSNumber)?.intValue ?? 0
                if Date() > deadline || size > 1_048_576 {
                    process.terminate()
                    // SIGTERM may be ignored; enforce the process deadline.
                    try await Task.sleep(nanoseconds: 100_000_000)
                    if process.isRunning { kill(process.processIdentifier, SIGKILL) }
                    process.waitUntilExit()
                    throw SparkImportError(message: "Spark took too long or returned too much data. Try syncing again.")
                }
                try await Task.sleep(nanoseconds: 50_000_000)
            }
            guard process.terminationStatus == 0 else {
                throw SparkImportError(message: "Spark could not respond. Keep Spark open and enable CLI Read access in its AI Agents settings.")
            }
            let data = try Data(contentsOf: output)
            guard data.count <= 1_048_576, let text = String(data: data, encoding: .utf8) else {
                throw SparkImportError(message: "Spark returned an unreadable response.")
            }
            return text
        }.value
    }
}

struct SparkImportConfiguration: Codable {
    var enabled = false
    var vaultID = ""
    var since = ""
    var transcripts = false
    var namespace = UUID().uuidString
    var lastSync: Date?
    var importedIDs: Set<String>?
    var lastFullReview: Date?
}

enum SparkSyncPlan {
    static func fullReviewNeeded(last: Date?, now: Date) -> Bool {
        guard let last else { return true }
        return now.timeIntervalSince(last) >= 86_400
    }
    static func pending(_ ids: [String], known: Set<String>, fullReview: Bool) -> [String] {
        ids.filter { fullReview || !known.contains($0) }
    }
}

@MainActor
final class SparkMeetingImporter: ObservableObject {
    @Published private(set) var configuration: SparkImportConfiguration
    @Published private(set) var busy = false
    @Published private(set) var status = "Connect Spark to import your meeting notes."
    private let defaults: UserDefaults
    private let key = "sparkMeetingImport.v1"
    private var timer: Task<Void, Never>?
    private var node: HiveNode?

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        configuration = defaults.data(forKey: key).flatMap { try? JSONDecoder().decode(SparkImportConfiguration.self, from: $0) } ?? SparkImportConfiguration()
    }
    func start(node: HiveNode) {
        guard timer == nil else { return }
        self.node = node
        timer = Task { [weak self] in
            while !Task.isCancelled {
                if let self, self.configuration.enabled { await self.sync() }
                do { try await Task.sleep(nanoseconds: 300_000_000_000) } catch { break }
            }
        }
    }
    func checkConnection() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do {
            _ = try SparkMeetingFormat.page(await SparkMeetingCLI.run(["meetings", "--page-size", "1"]))
            status = "Spark is ready. Choose a Vault and start importing."
        } catch { status = error.localizedDescription }
    }
    func resume() {
        guard !configuration.vaultID.isEmpty, !busy else { return }
        configuration.enabled = true
        do { try save(); Task { await sync() } } catch { status = "Could not save Spark settings." }
    }
    deinit { timer?.cancel() }
    private func save() throws { defaults.set(try JSONEncoder().encode(configuration), forKey: key) }
    func connect(vaultID: String, days: Int, transcripts: Bool) {
        guard !busy, UUID(uuidString: vaultID) != nil, [7, 30, 90, 365].contains(days) else { return }
        let formatter = DateFormatter(); formatter.dateFormat = "yyyy/MM/dd"; formatter.locale = Locale(identifier: "en_US_POSIX")
        configuration.importedIDs = []
        configuration.lastFullReview = nil
        configuration.lastSync = nil
        configuration.vaultID = vaultID
        configuration.since = formatter.string(from: Date().addingTimeInterval(-Double(days) * 86400))
        configuration.transcripts = transcripts
        configuration.enabled = true
        do { try save(); Task { await sync() } } catch { status = "Could not save Spark settings." }
    }
    func pause() {
        configuration.enabled = false
        try? save()
        status = busy ? "Stopping after the current meeting…" : "Paused. Imported notes remain in your Library."
    }
    func sync(refreshExisting: Bool = false) async {
        guard !busy, configuration.enabled, let node else { return }
        busy = true
        defer { busy = false }
        let config = configuration
        let fullReview = refreshExisting || SparkSyncPlan.fullReviewNeeded(last: config.lastFullReview, now: Date())
        var known = config.importedIDs ?? []
        do {
            _ = try await Task.detached(priority: .utility) { try node.vaultOpen() }.value
            let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/ohhive/spark-import/\(config.namespace)")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
            var page = 1; var pages = 1; var seen = Set<String>(); var imported = 0
            repeat {
                guard configuration.enabled else { return }
                status = "Checking Spark meetings…"
                let listing = try await SparkMeetingCLI.run(["meetings", "--filter", "after:\(config.since)", "--page", String(page), "--page-size", "50"])
                let parsed = try SparkMeetingFormat.page(listing)
                pages = parsed.pages
                let unique = parsed.ids.filter { seen.insert($0).inserted }
                let pending = SparkSyncPlan.pending(unique, known: known, fullReview: fullReview)
                for id in pending {
                    guard configuration.enabled else { return }
                    var args = ["meeting", "--notes"]
                    if config.transcripts { args.append("--transcript") }
                    args.append(id)
                    let text = try await SparkMeetingCLI.run(args)
                    guard configuration.enabled else { return }
                    let markdown = try SparkMeetingFormat.markdown(text, id: id)
                    let file = root.appendingPathComponent("spark-\(config.namespace)-\(id).md")
                    try Data(markdown.utf8).write(to: file, options: .atomic)
                    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
                    let receipt = try await Task.detached(priority: .utility) {
                        try node.vaultIntakeApproveFile(vaultId: config.vaultID, root: root.path, relativePath: file.lastPathComponent, project: "meetings")
                    }.value
                    known.insert(id)
                    configuration.importedIDs = known
                    try save()
                    if !receipt.unchanged { imported += 1 }
                    status = fullReview ? "Refreshing existing notes; \(imported) added or updated." : "Importing new meetings; \(imported) added."
                }
                page += 1
            } while page <= pages
            guard configuration.enabled else { return }
            if fullReview { configuration.lastFullReview = Date() }
            configuration.lastSync = Date(); try save()
            status = fullReview ? "Refresh complete. \(imported) meetings added or updated." : "Up to date. \(imported) new meetings imported."
        } catch {
            status = "Sync stopped: \(error.localizedDescription) Earlier imports are saved; the next sync will retry."
        }
    }
}
