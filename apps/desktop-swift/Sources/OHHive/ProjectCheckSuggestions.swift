import Foundation

struct ProjectCheckSuggestion: Identifiable {
    var id: String { command + "|" + args.joined(separator: "|") }
    let name: String
    let explanation: String
    let evidence: String
    let command: String
    let args: [String]
}
struct ProjectCheckReport {
    var checks: [ProjectCheckSuggestion] = []
    var notes: [String] = []
}
enum ProjectCheckInspectionError: LocalizedError {
    case invalidRepository, oversized, invalidMetadata
    var errorDescription: String? {
        switch self {
        case .invalidRepository: "Choose a GitHub repository before finding checks."
        case .oversized: "Repository metadata is too large for this quick inspection. Add checks under Advanced."
        case .invalidMetadata: "Repository configuration could not be read. Add checks under Advanced."
        }
    }
}
/// Read-only, curated detection. No script execution, dependency installation or model calls.
enum ProjectCheckInspector {
    private struct Entry: Decodable { let name: String; let type: String; let sha: String; let size: Int? }
    private struct Blob: Decodable { let encoding: String; let content: String }
    static func inspect(repository: String, reference: String?, fetch: (String) async throws -> Data) async throws -> ProjectCheckReport {
        guard let repo = URLComponents(string: repository), repo.scheme == "https", repo.host == "github.com",
              repo.user == nil, repo.password == nil, repo.query == nil, repo.fragment == nil, repo.port == nil else { throw ProjectCheckInspectionError.invalidRepository }
        let parts = repo.path.split(separator: "/").map(String.init)
        guard parts.count == 2, parts.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." && $0.utf8.allSatisfy { (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || [45,46,95].contains($0) } }) else { throw ProjectCheckInspectionError.invalidRepository }
        let name = parts[1].hasSuffix(".git") ? String(parts[1].dropLast(4)) : parts[1]
        guard !name.isEmpty else { throw ProjectCheckInspectionError.invalidRepository }
        let base = "https://api.github.com/repos/\(parts[0])/\(name)"
        var url = URLComponents(string: base + "/contents")!
        if let reference, !reference.isEmpty { url.queryItems = [URLQueryItem(name: "ref", value: reference)] }
        let data = try await fetch(url.url!.absoluteString)
        guard data.count <= 2_000_000 else { throw ProjectCheckInspectionError.oversized }
        let entries = try JSONDecoder().decode([Entry].self, from: data)
        guard entries.count < 1000 else { throw ProjectCheckInspectionError.oversized }
        var package: Data?
        if let file = entries.first(where: { $0.name == "package.json" && $0.type == "file" }) {
            guard let size = file.size, size <= 128_000, size >= 0,
                  file.sha.count == 40, file.sha.allSatisfy({ $0.isHexDigit }) else { throw ProjectCheckInspectionError.oversized }
            let raw = try await fetch(base + "/git/blobs/" + file.sha)
            guard raw.count <= 256_000 else { throw ProjectCheckInspectionError.oversized }
            let blob = try JSONDecoder().decode(Blob.self, from: raw)
            guard blob.encoding == "base64", let decoded = Data(base64Encoded: blob.content.filter { !$0.isWhitespace }), decoded.count <= 128_000 else { throw ProjectCheckInspectionError.invalidMetadata }
            package = decoded
        }
        return try recommend(files: Set(entries.filter { $0.type == "file" }.map(\.name)), directories: Set(entries.filter { $0.type == "dir" }.map(\.name)), package: package)
    }
    static func recommend(files: Set<String>, directories: Set<String>, package: Data?) throws -> ProjectCheckReport {
        var result = ProjectCheckReport()
        if let package {
            guard let json = try JSONSerialization.jsonObject(with: package) as? [String: Any] else { throw ProjectCheckInspectionError.invalidMetadata }
            let scripts = json["scripts"] as? [String: String] ?? [:]
            let declared = (json["packageManager"] as? String)?.split(separator: "@").first.map(String.init)
            var managers = Set<String>()
            if files.contains("package-lock.json") || files.contains("npm-shrinkwrap.json") { managers.insert("npm") }
            if files.contains("pnpm-lock.yaml") { managers.insert("pnpm") }
            if files.contains("yarn.lock") { managers.insert("yarn") }
            if files.contains("bun.lock") || files.contains("bun.lockb") { managers.insert("bun") }
            if let declared { managers.insert(declared) }
            if managers.count == 1, let manager = managers.first, ["npm", "pnpm", "yarn"].contains(manager) {
                for (key, name, why) in [("test", "Run existing tests", "Catch behavior that this change might break."), ("build", "Build the project", "Check that the project can be built."), ("typecheck", "Check types", "Catch incompatible values and interfaces."), ("lint", "Check code quality", "Run the project's configured code-quality rules.")] {
                    if let script = scripts[key], !script.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        if script.contains("no test specified") || script.contains("--watch") || script.contains("--fix") || (script.contains("vitest") && !script.contains(" run") && !script.contains("--run")) || script.contains("react-scripts test") {
                            result.notes.append("The \(key) script appears to be a placeholder, continuous watcher or auto-fix command. Review it under Advanced before using it.")
                        } else {
                            result.checks.append(.init(name:name, explanation:why, evidence:"package.json → scripts.\(key); \(manager) project configuration", command:manager, args:["run", key]))
                        }
                    }
                }
            } else { result.notes.append("JavaScript project found, but its package manager is missing, conflicting or unsupported. Choose the correct command under Advanced.") }
        }
        if files.contains("Cargo.toml") {
            result.checks.append(.init(name:"Check Rust compilation", explanation:"Check that the Rust project compiles.", evidence:"Cargo.toml", command:"cargo", args:["check"]))
            result.checks.append(.init(name:"Run Rust tests", explanation:"Run declared Rust tests; a successful run may still contain zero tests.", evidence:"Cargo.toml", command:"cargo", args:["test"]))
        }
        if files.contains("Package.swift") {
            result.checks.append(.init(name:"Build Swift package", explanation:"Check that the package builds on this computer.", evidence:"Package.swift", command:"swift", args:["build"]))
            if directories.contains("Tests") { result.checks.append(.init(name:"Run Swift package tests", explanation:"Run the package's configured tests.", evidence:"Package.swift and Tests/", command:"swift", args:["test"])) }
        }
        if files.contains("pyproject.toml") || files.contains("requirements.txt") { result.notes.append("Python project found. Its test runner and environment need confirming before adding a command.") }
        if result.checks.isEmpty { result.notes.append("No supported automatic checks found at the repository root. Ask the agent to add tests for the requested behavior, or add a known command under Advanced.") }
        result.notes.append("These commands were found in configuration, not tested. Required tools and dependencies must be installed. Nested projects are not inspected.")
        return result
    }
}
