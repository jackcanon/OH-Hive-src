import Foundation
import OHHiveFFI

/// App-owned worker: closing Settings does not interrupt execution. Starting is explicit each launch.
@MainActor
final class PrivateCodingWorkerModel: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var status = "Coding worker is off on this Mac."
    private let node: HiveNode
    private var session: PrivateCodingWorker?
    private var work: Task<Void, Never>?
    private var readiness: Task<Void, Never>?
    private var generation = UUID()

    init(node: HiveNode) { self.node = node }
    deinit { work?.cancel(); readiness?.cancel() }

    func stop() {
        generation = UUID()
        enabled = false
        session?.stop()
        if let session { Task { try? await session.advertise(gitConnected: false) } }
        work?.cancel(); readiness?.cancel()
        // Do not discard an executing operation: Rust retains its session until cooperative cleanup.
        session = nil
        status = "Stopping any active task. New work is disabled."
    }

    func start(github: GitHubAuthManager) {
        guard !enabled else { return }
        generation = UUID()
        let current = generation
        enabled = true
        status = "Connecting to your primary…"
        work = Task { [weak self] in
            guard let self else { return }
            do {
                let session = try await node.privateCodingWorkerOpen()
                guard generation == current, !Task.isCancelled else { session.stop(); return }
                self.session = session
                readiness = Task { [weak self] in
                    while !Task.isCancelled {
                        do { try await session.advertise(gitConnected: github.isConnected) }
                        catch {
                            guard let self, self.generation == current else { return }
                            self.status = "Could not report readiness: \(Self.message(error))"
                        }
                        do { try await Task.sleep(for: .seconds(15)) } catch { return }
                    }
                }
                while !Task.isCancelled, generation == current {
                    let next = try await session.nextWork()
                    if next == "prepare" {
                        status = "Preparing a repository on this Mac…"
                        var result = ""
                        if github.isConnected {
                            try await github.withRepositoryGitToken { token in result = try await session.tick(token: token) }
                        } else {
                            result = try await session.tick(token: "")
                        }
                        guard generation == current else { return }
                        status = result
                    } else if next == "run" {
                        status = "Running a coding task on this Mac…"
                        let result = try await session.tick(token: "")
                        guard generation == current else { return }
                        status = result
                    } else { status = "Ready. Waiting for a task from your primary." }
                    do { try await Task.sleep(for: .seconds(3)) } catch { return }
                }
            } catch {
                guard generation == current else { return }
                stop()
                status = "Worker paused: \(Self.message(error)) Restart after resolving the issue."
            }
        }
    }
    private static func message(_ error: Error) -> String {
        if let hive = error as? HiveError, case .Failed(let message) = hive { return message }
        return error.localizedDescription
    }
}
