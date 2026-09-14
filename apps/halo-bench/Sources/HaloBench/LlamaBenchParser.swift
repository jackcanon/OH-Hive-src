import Foundation

/// Parses `llama-bench`'s markdown result table, e.g.
/// `| qwen3 14B Q4_K - Medium | 8.38 GiB | 14.77 B | Metal,RPC | 16 | 8/7.7 | pp512 | 240.20 ± 1.23 |`
/// Column count varies by build/flags, so rows are matched by shape: a `t/s` cell last, a
/// test-name cell (`pp512`, `tg128`, `tg1024`...) just before it.
enum LlamaBenchParser {
    static func rows(in text: String) -> [BenchRow] {
        var out: [BenchRow] = []
        for line in text.split(separator: "\n") {
            let s = line.trimmingCharacters(in: .whitespaces)
            guard s.hasPrefix("|"), !s.contains("---"), !s.lowercased().contains("| model") else { continue }
            let cells = s.split(separator: "|", omittingEmptySubsequences: false)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .dropFirst().dropLast()
            guard cells.count >= 5 else { continue }
            let tps = Array(cells).last!
            let test = cells[cells.index(cells.endIndex, offsetBy: -2)]
            guard test.range(of: #"^(pp|tg)\d+"#, options: .regularExpression) != nil else { continue }
            let parts = tps.replacingOccurrences(of: "±", with: " ").split(separator: " ").map(String.init)
            guard let v = parts.first.flatMap(Double.init) else { continue }
            let sd = parts.count > 1 ? Double(parts[1]) : nil
            let c = Array(cells)
            out.append(BenchRow(model: c[0], size: c.count > 1 ? c[1] : "", backend: c.count > 3 ? c[3] : "",
                                test: test, tokensPerSec: v, stddev: sd))
        }
        return out
    }

    /// Well-known failure signatures from Test 01, so the report can say *why* in plain words.
    static func diagnose(_ log: String) -> String? {
        let l = log.lowercased()
        if l.contains("unsupported op") && l.contains("blas") {
            return "A worker ran the graph on its BLAS/CPU backend instead of the GPU (`ggml_backend_blas_graph_compute: unsupported op`). The worker was started without `-d MTL0`/`-d CUDA0` -- a harness/config problem, not a model result. Check the host's Worker device field in Fleet."
        }
        if l.contains("no route to host") {
            return "\"No route to host\" on a LAN address that pings fine means macOS Local Network permission is denied for HaloBench. System Settings -> Privacy & Security -> Local Network -> enable HaloBench, then rerun."
        }
        if l.contains("res = -3") || l.contains("insufficient memory") || l.contains("status 5") {
            return "Metal reported insufficient memory at compute (`res = -3`). Either a shard exceeded that node's real usable budget (~65% of RAM), or -- if every node was inside budget and no worker compiled a kernel -- the Q8_0-over-RPC hypothesis from Test 01."
        }
        if l.contains("failed to connect to") {
            return "Host could not reach an RPC worker. Check the worker is running, bound to its *wired* IP, and that Wi-Fi is off on wired nodes (AP client isolation drops TCP silently)."
        }
        if l.contains("remote rpc server crashed") || l.contains("malformed response") {
            return "A worker crashed mid-run (SIGPIPE / malformed response). Most often the worker was handed a share it could not hold -- confirm the tensor-split is RPC-first, local-last."
        }
        if l.contains("unknown model architecture") || l.contains("unknown architecture") {
            return "This llama.cpp tag does not know the model's architecture -- a software limit, not a hardware result."
        }
        if l.contains("unsuccessful graph computations are not supported with rpc") {
            return "Worker asserted after a failed compute; see the first Metal/CUDA error above it for the real cause."
        }
        return nil
    }
}
