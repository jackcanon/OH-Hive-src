import Foundation

/// Reads and writes the markdown reports in `docs/halo-reports/`. One file per run, named
/// `YYYY-MM-DD_HH-mm_<slug>.md`, with a small key/value header the app parses back. The folder
/// is the source of truth; the app's History is a view of it. Raw logs go alongside as `.log`.
struct ReportStore {
    var directory: URL

    static let headerKeys = ["date", "outcome", "model", "placement", "tensor_split", "pp512", "tg128"]

    // MARK: Read

    func loadAll() -> [TestReport] { (try? loadAllThrowing()) ?? [] }

    func loadAllThrowing() throws -> [TestReport] {
        let items = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
        return items.filter { $0.pathExtension == "md" }
            .compactMap { parse(url: $0) }
            .sorted { $0.date > $1.date }
    }

    /// Where reports go if the configured folder can't be written (e.g. macOS denies the app
    /// access to an external volume). Never lose a run over a permission dialog.
    static var fallbackDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("HaloBench/reports", isDirectory: true)
    }

    func parse(url: URL) -> TestReport? {
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        var title = url.deletingPathExtension().lastPathComponent
        var kv: [String: String] = [:]
        var summary = ""
        var inSummary = false
        for raw in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let line = String(raw)
            if line.hasPrefix("# ") && title == url.deletingPathExtension().lastPathComponent { title = String(line.dropFirst(2)); continue }
            if line.hasPrefix("- "), let colon = line.firstIndex(of: ":") {
                let k = line[line.index(line.startIndex, offsetBy: 2)..<colon].trimmingCharacters(in: .whitespaces)
                if Self.headerKeys.contains(k) { kv[k] = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces); continue }
            }
            if line.hasPrefix("## ") { inSummary = line.lowercased().contains("summary"); continue }
            if inSummary, !line.isEmpty { summary += (summary.isEmpty ? "" : " ") + line }
        }
        let date = kv["date"].flatMap { Self.iso.date(from: $0) } ?? (try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
        return TestReport(fileURL: url, title: title, date: date,
                          outcome: Outcome(rawValue: kv["outcome"] ?? "") ?? .partial,
                          model: kv["model"] ?? "", placement: kv["placement"] ?? "",
                          tensorSplit: kv["tensor_split"] ?? "",
                          pp512: kv["pp512"].flatMap(Double.init), tg128: kv["tg128"].flatMap(Double.init),
                          summary: summary, body: text)
    }

    // MARK: Write

    /// Files a report and its raw log. Returns the markdown URL.
    @discardableResult
    func file(title: String, date: Date, outcome: Outcome, model: String, placement: String, tensorSplit: String,
              rows: [BenchRow], summary: String, notes: String, command: String, log: String) throws -> URL {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let stamp = Self.fileStamp.string(from: date)
        let slug = Self.slug(title)
        let base = "\(stamp)_\(slug)"
        let md = directory.appendingPathComponent(base + ".md")
        let logURL = directory.appendingPathComponent(base + ".log")
        let pp = rows.first { $0.test == "pp512" }?.tokensPerSec
        let tg = rows.first { $0.test == "tg128" }?.tokensPerSec

        var t = "# \(title)\n\n"
        t += "- date: \(Self.iso.string(from: date))\n"
        t += "- outcome: \(outcome.rawValue)\n"
        t += "- model: \(model)\n"
        t += "- placement: \(placement)\n"
        t += "- tensor_split: \(tensorSplit)\n"
        if let pp { t += "- pp512: \(Self.num(pp))\n" }
        if let tg { t += "- tg128: \(Self.num(tg))\n" }
        t += "- filed_by: HaloBench \(AppInfo.version)\n\n"
        t += "## Summary\n\n\(summary)\n\n"
        if !rows.isEmpty {
            t += "## Results\n\n| test | tok/s | ± |\n|---|---:|---:|\n"
            for r in rows { t += "| \(r.test) | \(Self.num(r.tokensPerSec)) | \(r.stddev.map(Self.num) ?? "") |\n" }
            t += "\n"
        }
        if !notes.isEmpty { t += "## Notes\n\n\(notes)\n\n" }
        t += "## Command\n\n```\n\(command)\n```\n\n"
        t += "Raw log: `\(base).log`\n"
        try t.write(to: md, atomically: true, encoding: .utf8)
        try log.write(to: logURL, atomically: true, encoding: .utf8)
        return md
    }

    // MARK: Helpers

    static let iso: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter(); f.formatOptions = [.withInternetDateTime]; return f
    }()
    static let fileStamp: DateFormatter = {
        let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd_HH-mm"; f.locale = Locale(identifier: "en_US_POSIX"); return f
    }()
    static func num(_ v: Double) -> String { String(format: v == v.rounded() ? "%.0f" : "%.1f", v) }
    static func slug(_ s: String) -> String {
        let allowed = CharacterSet.alphanumerics
        var out = ""; var lastDash = false
        for ch in s.lowercased() {
            if ch.unicodeScalars.allSatisfy(allowed.contains) { out.append(ch); lastDash = false }
            else if !lastDash { out.append("-"); lastDash = true }
        }
        return String(out.trimmingCharacters(in: CharacterSet(charactersIn: "-")).prefix(60))
    }
}

enum AppInfo {
    static let version = "0.1.0"
}
