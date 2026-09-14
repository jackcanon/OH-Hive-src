import Foundation

/// Persisted app settings: `~/Library/Application Support/HaloBench/config.json`.
struct Config: Codable {
    var fleet: [Host] = DefaultFleet.hosts
    /// Which fleet entry is *this* machine (the host that runs llama-bench). Nil = unknown.
    var localHostID: UUID? = nil
    var llamaBenchPath: String = "~/halo/llama.cpp/build/bin/llama-bench"
    var modelsDir: String = "~/halo/models"
    /// The repo folder reports are filed into. Source of truth for History.
    var reportsDir: String = Config.defaultReportsDir
    var llamaTag: String = "b10883"

    static let defaultReportsDir = "/Volumes/10TB JBOD/Agents/Claude/Projects/Apps/OH Cloud-src/docs/halo-reports"

    static var fileURL: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("HaloBench", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base.appendingPathComponent("config.json")
    }

    static func load() -> Config {
        guard let d = try? Data(contentsOf: fileURL), let c = try? JSONDecoder().decode(Config.self, from: d) else { return Config() }
        return c
    }

    func save() {
        let e = JSONEncoder(); e.outputFormatting = [.prettyPrinted, .sortedKeys]
        try? e.encode(self).write(to: Config.fileURL)
    }

    var reportsURL: URL { URL(fileURLWithPath: (reportsDir as NSString).expandingTildeInPath) }
    var llamaBenchURL: URL { URL(fileURLWithPath: (llamaBenchPath as NSString).expandingTildeInPath) }
}

extension String {
    var expandingTilde: String { (self as NSString).expandingTildeInPath }
}
