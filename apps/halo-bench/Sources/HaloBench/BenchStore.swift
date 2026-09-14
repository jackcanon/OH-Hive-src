import Foundation
import Observation

@MainActor
@Observable
final class BenchStore {
    var config: Config { didSet { config.save() } }
    var reports: [TestReport] = []
    var plan: TestPlan = Presets.all[0].build(DefaultFleet.hosts)
    var log: [String] = []
    var phase: Phase = .idle
    var lastResult: Runner.Result?
    var lastFiled: URL?
    var errorMessage: String?
    var timeline = Timeline()
    var lastOutputAt: Date?

    private var runner: Runner?

    enum Phase: Equatable { case idle, running, finished(Outcome) }

    var isLoadingHistory = false
    var historyError: String?

    init() {
        config = Config.load()
        plan = Presets.all[0].build(config.fleet)
        reload()
    }

    // MARK: History

    /// Reads the reports folder off the main thread -- it lives on an external volume, and a
    /// stalled read there must never keep the window from appearing.
    func reload() {
        let dir = config.reportsURL
        isLoadingHistory = true
        Task.detached(priority: .userInitiated) {
            var err: String? = nil
            var r: [TestReport] = []
            do { r = try ReportStore(directory: dir).loadAllThrowing() }
            catch { err = Self.describe(error, dir: dir) }
            // Also pick up anything that landed in the fallback folder.
            let fb = ReportStore(directory: ReportStore.fallbackDirectory).loadAll()
            let merged = (r + fb).sorted { $0.date > $1.date }
            await MainActor.run {
                self.reports = merged; self.historyError = err; self.isLoadingHistory = false
                // Keep the Run tab pointed at the next unfinished preset unless the user has edited the plan.
                if self.phase == .idle, Presets.all.contains(where: { $0.build([]).title == self.plan.title }) {
                    self.plan = Presets.next(given: merged).build(self.config.fleet)
                }
            }
        }
    }

    nonisolated static func describe(_ error: Error, dir: URL) -> String {
        let e = error as NSError
        if e.domain == NSCocoaErrorDomain, e.code == NSFileReadNoPermissionError || e.code == NSFileReadNoSuchFileError || e.code == 257 {
            return "macOS is not letting HaloBench read \(dir.path). If it's on an external drive: System Settings → Privacy & Security → Files and Folders → HaloBench → allow Removable Volumes (or grant Full Disk Access), then Reload."
        }
        return "Couldn't read \(dir.path): \(e.localizedDescription)"
    }

    var passCount: Int { reports.filter { $0.outcome == .pass }.count }
    var failCount: Int { reports.filter { $0.outcome == .fail }.count }
    var bestDecode: TestReport? { reports.filter { $0.tg128 != nil }.max { ($0.tg128 ?? 0) < ($1.tg128 ?? 0) } }
    var latest: TestReport? { reports.first }

    // MARK: Run

    func start() {
        guard phase != .running else { return }
        let title = plan.title.trimmingCharacters(in: .whitespaces)
        guard !title.isEmpty else { errorMessage = "Give the test a title -- it becomes the report's filename."; return }
        errorMessage = nil
        log = []
        phase = .running
        lastFiled = nil
        timeline = .fresh(isSplit: plan.isSplit)
        lastOutputAt = Date()
        let r = Runner()
        runner = r
        let plan = self.plan
        let config = self.config
        Task {
            let result = await r.run(plan: plan, config: config) { [weak self] line in self?.observe(line) }
            await self.finish(result: result, plan: plan, cancelled: r.isCancelled)
        }
    }

    private func observe(_ line: String) {
        log.append(line)
        lastOutputAt = Date()
        timeline.observe(line)
    }

    func cancel() { runner?.cancel() }

    private func finish(result: Runner.Result, plan: TestPlan, cancelled: Bool) async {
        lastResult = result
        let pp = result.rows.first { $0.test == "pp512" }?.tokensPerSec
        let tg = result.rows.first { $0.test.hasPrefix("tg") }?.tokensPerSec
        let outcome: Outcome = cancelled ? .aborted : (result.exitCode == 0 && tg != nil) ? .pass : (tg != nil ? .partial : .fail)
        phase = .finished(outcome)

        let fleet = config.fleet
        let workerNames = plan.workers.compactMap { w in fleet.first { $0.id == w.hostID } }
        let localName = plan.host(in: fleet)?.name
            ?? config.localHostID.flatMap { id in fleet.first { $0.id == id }?.name }
            ?? Host.currentMachineName
        let placement = plan.isSplit
            ? "\(localName) host + " + zip(workerNames, plan.workers).map { "\($0.name) \(ReportStore.num($1.shareGB)) GB\($0.managed ? "" : " (remote)")" }.joined(separator: ", ")
            : "\(localName) alone"
        let modelName = (plan.modelPath as NSString).lastPathComponent
        let secs = Int(result.endedAt.timeIntervalSince(result.startedAt))

        var summary: String
        switch outcome {
        case .pass:
            summary = "\(modelName) ran on \(placement)"
            if plan.isSplit { summary += " (tensor-split \(plan.tensorSplit), RPC-first)" }
            summary += ". Decode \(tg.map(ReportStore.num) ?? "?") tok/s"
            if let pp { summary += ", prefill \(ReportStore.num(pp)) tok/s" }
            summary += " (median of \(plan.repetitions), -p \(plan.promptTokens) -n \(plan.genTokens)). Took \(secs)s including load."
        case .partial:
            summary = "\(modelName) on \(placement) produced numbers but llama-bench exited \(result.exitCode). Treat with care -- see the log."
        case .fail:
            summary = "\(modelName) on \(placement) did not complete (exit \(result.exitCode) after \(secs)s)."
            if let why = LlamaBenchParser.diagnose(result.log) { summary += " " + why }
        case .aborted:
            summary = "Run aborted by hand after \(secs)s."
        }

        let notes = plan.notes + (plan.notes.isEmpty ? "" : "\n\n") + "llama.cpp tag: \(config.llamaTag)"
        func write(to dir: URL) throws -> URL {
            try ReportStore(directory: dir).file(
                title: plan.title, date: result.startedAt, outcome: outcome, model: modelName, placement: placement,
                tensorSplit: plan.isSplit ? plan.tensorSplit : "", rows: result.rows, summary: summary,
                notes: notes, command: result.command, log: result.log)
        }
        do {
            let url = try write(to: config.reportsURL)
            lastFiled = url
            observe("==> report filed: \(url.lastPathComponent)")
        } catch {
            observe("!! could not write to \(config.reportsDir): \(error.localizedDescription)")
            if let url = try? write(to: ReportStore.fallbackDirectory) {
                lastFiled = url
                observe("==> report filed to fallback folder instead: \(url.path)")
                errorMessage = "The reports folder isn't writable (\(Self.describe(error, dir: config.reportsURL))). The report was saved to \(ReportStore.fallbackDirectory.path) instead — move it into the repo once access is fixed."
            } else {
                errorMessage = "Could not write the report anywhere: \(error.localizedDescription)"
            }
        }
        if outcome == .aborted { timeline.fail("aborted by hand") }
        reload()
    }

    // MARK: Import an externally-run log

    /// Files a report from a llama-bench log produced outside the app (hand-run over SSH, etc.).
    func importLog(url: URL, title: String) {
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { errorMessage = "Could not read \(url.lastPathComponent)"; return }
        let rows = LlamaBenchParser.rows(in: text)
        let tg = rows.first { $0.test.hasPrefix("tg") }?.tokensPerSec
        let outcome: Outcome = tg != nil ? .pass : .fail
        let date = (try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? Date()
        var summary = outcome == .pass
            ? "Imported from \(url.lastPathComponent). Decode \(tg.map(ReportStore.num) ?? "?") tok/s."
            : "Imported from \(url.lastPathComponent); no result table found."
        if outcome == .fail, let why = LlamaBenchParser.diagnose(text) { summary += " " + why }
        do {
            lastFiled = try ReportStore(directory: config.reportsURL).file(
                title: title, date: date, outcome: outcome, model: rows.first?.model ?? "", placement: "(imported)",
                tensorSplit: "", rows: rows, summary: summary, notes: "", command: "(external run)", log: text)
            reload()
        } catch { errorMessage = error.localizedDescription }
    }

    // MARK: Presets

    func apply(_ preset: Preset) {
        plan = preset.build(config.fleet)
    }
}

extension Host {
    static var currentMachineName: String {
        Foundation.Host.current().localizedName ?? ProcessInfo.processInfo.hostName
    }
}
