import Foundation

/// The milestones of one run, in order. Derived from the log stream so the timeline shows exactly
/// where a run is -- and, on failure, exactly where it died.
enum Stage: Int, CaseIterable, Identifiable {
    case workers, launch, hostGPU, load, prefill, decode, report
    var id: Int { rawValue }

    var title: String {
        switch self {
        case .workers: "Workers"
        case .launch: "Launch"
        case .hostGPU: "Host GPU"
        case .load: "Load"
        case .prefill: "Prefill"
        case .decode: "Decode"
        case .report: "Report"
        }
    }
    var hint: String {
        switch self {
        case .workers: "restart ggml-rpc-server over SSH, wait for ports"
        case .launch: "start llama-bench with the RPC-first split"
        case .hostGPU: "Metal/CUDA init on this Mac"
        case .load: "read the GGUF, ship each worker its share, run the prompt reps (quiet: minutes for big models)"
        case .prefill: "prompt processing result"
        case .decode: "token generation, 3 reps -- the number Halo cares about"
        case .report: "parse results, file the markdown"
        }
    }
}

enum StageStatus: Equatable { case pending, active, done, failed, skipped }

struct StageState: Equatable {
    var status: StageStatus = .pending
    var startedAt: Date?
    var endedAt: Date?
    var detail: String = ""
    var duration: TimeInterval? {
        guard let s = startedAt else { return nil }
        return (endedAt ?? Date()).timeIntervalSince(s)
    }
}

/// Watches log lines and advances the stage machine.
struct Timeline: Equatable {
    var stages: [Stage: StageState] = Dictionary(uniqueKeysWithValues: Stage.allCases.map { ($0, StageState()) })
    var startedAt: Date?
    var endedAt: Date?
    var current: Stage?

    static func fresh(isSplit: Bool) -> Timeline {
        var t = Timeline()
        t.startedAt = Date()
        if isSplit { t.begin(.workers) } else { t.stages[.workers]?.status = .skipped; t.begin(.launch) }
        return t
    }

    mutating func begin(_ s: Stage) {
        if let c = current, c != s, stages[c]?.status == .active { finish(c) }
        stages[s]?.status = .active
        stages[s]?.startedAt = Date()
        current = s
    }
    mutating func finish(_ s: Stage, detail: String? = nil) {
        guard stages[s]?.status == .active else { return }
        stages[s]?.status = .done
        stages[s]?.endedAt = Date()
        if let d = detail { stages[s]?.detail = d }
    }
    mutating func fail(_ detail: String) {
        let s = current ?? .launch
        stages[s]?.status = .failed
        stages[s]?.endedAt = Date()
        if stages[s]!.detail.isEmpty { stages[s]?.detail = detail }
        endedAt = Date()
    }
    mutating func complete() { endedAt = Date() }

    var elapsed: TimeInterval { guard let s = startedAt else { return 0 }; return (endedAt ?? Date()).timeIntervalSince(s) }
    var failedStage: Stage? { Stage.allCases.first { stages[$0]?.status == .failed } }

    /// Feed one log line. Order of checks matters -- most specific first.
    mutating func observe(_ line: String) {
        let t = line.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("!!") { fail(String(t.dropFirst(2)).trimmingCharacters(in: .whitespaces)); return }
        if t.hasPrefix("==> /") || t.contains("llama-bench -m") { finish(.workers); begin(.launch); return }
        if t.contains("is listening") { stages[.workers]?.detail = "workers up"; return }
        if t.hasPrefix("ggml_metal_device_init: GPU name") || t.contains("ggml_cuda_init: found") {
            finish(.launch); begin(.hostGPU); stages[.hostGPU]?.detail = t.components(separatedBy: "GPU name:").last?.trimmingCharacters(in: .whitespaces) ?? ""; return
        }
        if t.contains("recommendedMaxWorkingSetSize") {
            // llama-bench prints the results-table header *before* loading the model, so the
            // header is the start of Load, not the end of it. Load ends when the pp row lands
            // (the prefill reps themselves are folded into that same stretch).
            finish(.hostGPU); begin(.load)
            if let mb = t.split(separator: "=").last.map({ $0.trimmingCharacters(in: .whitespaces) }) { stages[.hostGPU]?.detail += "  budget \(mb)" }
            return
        }
        if t.hasPrefix("| model") { if current != .load { begin(.load) }; stages[.load]?.detail = "loading + shipping shards"; return }
        if t.hasPrefix("|"), t.range(of: #"\|\s*pp\d+\s*\|"#, options: .regularExpression) != nil {
            finish(.load, detail: "")
            begin(.prefill); finish(.prefill, detail: Self.tps(t))
            begin(.decode); return
        }
        if t.hasPrefix("|"), t.range(of: #"\|\s*tg\d+\s*\|"#, options: .regularExpression) != nil { finish(.decode, detail: Self.tps(t)); begin(.report); return }
        if t.contains("Remote RPC server crashed") || t.contains("ggml_abort") || t.contains("res = -3") || t.contains("Insufficient Memory") {
            fail(t); return
        }
        if t.hasPrefix("==> llama-bench exited") {
            if t.hasSuffix(" 0") { if current != .report { begin(.report) } }
            else if failedStage == nil { fail("llama-bench exited \(t.split(separator: " ").last ?? "")") }
            return
        }
        if t.hasPrefix("==> report filed") { finish(.report, detail: String(t.dropFirst(16))); complete(); return }
    }

    private static func tps(_ row: String) -> String {
        let cells = row.split(separator: "|").map { $0.trimmingCharacters(in: .whitespaces) }
        guard let last = cells.last(where: { !$0.isEmpty }) else { return "" }
        return last.split(separator: " ").first.map { "\($0) tok/s" } ?? ""
    }
}
