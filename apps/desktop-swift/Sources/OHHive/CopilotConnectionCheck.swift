import Foundation

struct CopilotCheckResult: Decodable, Sendable {
    let login: String?
    let models: [String]?
    let reply: String?
    let error: String?
}

enum CopilotConnectionCheck {
    static var executable: URL? {
        guard let root = Bundle.main.executableURL?.deletingLastPathComponent() else { return nil }
        let url = root.appendingPathComponent("hive-copilot-check")
        return FileManager.default.isExecutableFile(atPath: url.path) ? url : nil
    }

    /// Only the app reads Keychain; the helper receives one request via its private stdin pipe.
    static func run(token: String, login: String, model: String?) async throws -> CopilotCheckResult {
        guard let executable else { throw GitHubConnectorError.denied("Copilot checking is not included in this build.") }
        let input = try JSONSerialization.data(withJSONObject: ["token": token, "login": login, "model": model as Any? ?? NSNull()])
        return try await Task.detached(priority: .userInitiated) {
            try execute(input: input, executable: executable)
        }.value
    }

    private static func execute(input: Data, executable: URL) throws -> CopilotCheckResult {
        let process = Process()
        let stdin = Pipe(), stdout = Pipe()
        process.executableURL = executable
        process.standardInput = stdin
        process.standardOutput = stdout
        // Provider errors can contain sensitive context; only the helper's fixed errors reach UI.
        process.standardError = FileHandle.nullDevice
        process.environment = ProcessInfo.processInfo.environment.filter { ["PATH", "TMPDIR", "LANG", "LC_ALL"].contains($0.key) }
        try process.run()
        let deadline = DispatchWorkItem { if process.isRunning { process.terminate() } }
        DispatchQueue.global().asyncAfter(deadline: .now() + 120, execute: deadline)
        defer {
            deadline.cancel()
            if process.isRunning { process.terminate() }
        }
        try stdin.fileHandleForWriting.write(contentsOf: input)
        try stdin.fileHandleForWriting.close()
        // Drain while the child runs so a model catalog cannot fill the pipe and deadlock.
        let data = stdout.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0, data.count <= 1_048_576 else {
            throw GitHubConnectorError.denied("Copilot check did not finish. No automatic retry was made.")
        }
        return try JSONDecoder().decode(CopilotCheckResult.self, from: data)
    }
}
