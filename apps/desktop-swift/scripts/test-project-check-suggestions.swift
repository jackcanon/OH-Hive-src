// Compile alongside ProjectCheckSuggestions.swift; no network, credentials or app needed.
import Foundation
@main struct CheckSuggestionTests {
    static func main() async throws {
        let manifest = Data(#"{"scripts":{"test":"vitest run","build":"vite build","lint":"eslint ."},"packageManager":"pnpm@9.0.0"}"#.utf8)
        let report = try ProjectCheckInspector.recommend(files:["package.json", "pnpm-lock.yaml"], directories:[], package:manifest)
        precondition(report.checks.count == 3)
        precondition(report.checks.allSatisfy { $0.command == "pnpm" && $0.args.first == "run" })
        let conflict = try ProjectCheckInspector.recommend(files:["package-lock.json"], directories:[], package:manifest)
        precondition(conflict.checks.isEmpty)
        let absent = try ProjectCheckInspector.recommend(files:[], directories:[], package:Data(#"{"scripts":{"test":"test"}}"#.utf8))
        precondition(absent.checks.isEmpty)
        let placeholder = try ProjectCheckInspector.recommend(files:["package-lock.json"], directories:[], package:Data(#"{"scripts":{"test":"echo no test specified","lint":"eslint --fix .","build":"vite --watch"}}"#.utf8))
        precondition(placeholder.checks.isEmpty)
        let swift = try ProjectCheckInspector.recommend(files:["Package.swift"], directories:[], package:nil)
        precondition(swift.checks.count == 1)
        let swiftTests = try ProjectCheckInspector.recommend(files:["Package.swift"], directories:["Tests"], package:nil)
        precondition(swiftTests.checks.count == 2)
        let rust = try ProjectCheckInspector.recommend(files:["Cargo.toml"], directories:[], package:nil)
        precondition(rust.checks.map(\.args) == [["check"], ["test"]])
        let python = try ProjectCheckInspector.recommend(files:["pyproject.toml"], directories:[], package:nil)
        precondition(python.checks.isEmpty && python.notes.contains { $0.contains("Python") })
        var fetched: [String] = []
        let inspected = try await ProjectCheckInspector.inspect(repository:"https://github.com/owner/repo.git", reference:"feature/a&b") { url in
            fetched.append(url)
            if url.contains("/contents") { return Data(("[{\"name\":\"package.json\",\"type\":\"file\",\"sha\":\"" + String(repeating:"a",count:40) + "\",\"size\":100}]").utf8) }
            return try JSONSerialization.data(withJSONObject:["encoding":"base64", "content":manifest.base64EncodedString()])
        }
        precondition(inspected.checks.count == 3 && fetched.count == 2)
        precondition(URLComponents(string:fetched[0])?.queryItems?.first?.value == "feature/a&b")
        precondition(fetched[1].hasSuffix("/git/blobs/" + String(repeating:"a",count:40)))
        for bad in ["https://evil.test/o/r", "https://secret@github.com/o/r", "https://github.com/o/r?x=1", "https://github.com/o/../r"] {
            do { _ = try await ProjectCheckInspector.inspect(repository:bad, reference:nil) { _ in fatalError("invalid repository was fetched") }; fatalError("invalid URL accepted") } catch ProjectCheckInspectionError.invalidRepository { }
        }
        do { _ = try await ProjectCheckInspector.inspect(repository:"https://github.com/o/r", reference:nil) { _ in Data(repeating:32,count:2_000_001) }; fatalError("oversized accepted") } catch ProjectCheckInspectionError.oversized { }
        print("PASS: recommendation detection, manager conflicts, placeholders, project evidence, ref encoding, pinned blob, URL restrictions and response limit")
    }
}
