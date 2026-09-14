import Foundation

// MARK: - Fleet

/// One lab machine. Wired IP only -- Wi-Fi addresses measured 17-150 ms RTT with spikes to 490 ms
/// in Test 01 and silently wreck a run (HALO-TEST-01-LAN-SPLIT.md, "Wired IPs").
struct Host: Identifiable, Codable, Hashable {
    var id: UUID = UUID()
    var name: String
    var wiredIP: String
    var sshUser: String
    var chip: String
    var ramGB: Double
    /// Measured usable Metal/CUDA budget, not the spec sheet (~65% of RAM on Apple Silicon under RPC).
    var usableGB: Double
    var backend: Backend
    /// Path to ggml-rpc-server on that machine.
    var rpcServerPath: String
    var rpcPort: Int = 50052
    /// `-d` device the worker serves. Without it `ggml-rpc-server` may serve its BLAS/CPU device
    /// and crash at first compute with "unsupported op RMS_NORM" (HaloBench run 2026-09-10 09:00).
    var rpcDevice: String? = nil
    /// llama-bench on that machine, for when it's the *host* of a run driven from here over SSH.
    var llamaBenchPath: String? = nil
    /// False for machines HaloBench can't SSH into (a volunteer's box over Tailscale): the app
    /// skips the worker restart, waits for the port, and never pulls logs. The owner runs
    /// `ggml-rpc-server -H <tailscale-ip> -p 50052 -d MTL0|CUDA0` themselves (HALO-REMOTE-TESTER-SETUP.md).
    var managed: Bool = true
    var notes: String = ""

    enum Backend: String, Codable, CaseIterable { case metal = "Metal", cuda = "CUDA", cpu = "CPU" }

    init(id: UUID = UUID(), name: String, wiredIP: String, sshUser: String, chip: String, ramGB: Double, usableGB: Double,
         backend: Backend, rpcServerPath: String, rpcPort: Int = 50052, rpcDevice: String? = nil,
         llamaBenchPath: String? = nil, managed: Bool = true, notes: String = "") {
        self.id = id; self.name = name; self.wiredIP = wiredIP; self.sshUser = sshUser; self.chip = chip
        self.ramGB = ramGB; self.usableGB = usableGB; self.backend = backend; self.rpcServerPath = rpcServerPath
        self.rpcPort = rpcPort; self.rpcDevice = rpcDevice; self.llamaBenchPath = llamaBenchPath
        self.managed = managed; self.notes = notes
    }

    /// Tolerant of configs saved by older builds (fields added later default instead of failing).
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(UUID.self, forKey: .id) ?? UUID()
        name = try c.decode(String.self, forKey: .name)
        wiredIP = try c.decodeIfPresent(String.self, forKey: .wiredIP) ?? ""
        sshUser = try c.decodeIfPresent(String.self, forKey: .sshUser) ?? "jack"
        chip = try c.decodeIfPresent(String.self, forKey: .chip) ?? ""
        ramGB = try c.decodeIfPresent(Double.self, forKey: .ramGB) ?? 0
        usableGB = try c.decodeIfPresent(Double.self, forKey: .usableGB) ?? 0
        backend = try c.decodeIfPresent(Backend.self, forKey: .backend) ?? .metal
        rpcServerPath = try c.decodeIfPresent(String.self, forKey: .rpcServerPath) ?? ""
        rpcPort = try c.decodeIfPresent(Int.self, forKey: .rpcPort) ?? 50052
        rpcDevice = try c.decodeIfPresent(String.self, forKey: .rpcDevice)
        llamaBenchPath = try c.decodeIfPresent(String.self, forKey: .llamaBenchPath)
        managed = try c.decodeIfPresent(Bool.self, forKey: .managed) ?? true
        notes = try c.decodeIfPresent(String.self, forKey: .notes) ?? ""
    }

    var rpcEndpoint: String { "\(wiredIP):\(rpcPort)" }
    var effectiveDevice: String {
        if let d = rpcDevice, !d.isEmpty { return d }
        switch backend { case .metal: return "MTL0"; case .cuda: return "CUDA0"; case .cpu: return "CPU" }
    }
}

/// Fleet as probed 2026-09-09. Editable in the Fleet tab; persisted to the config file.
enum DefaultFleet {
    static let hosts: [Host] = [
        Host(name: "Midgaard", wiredIP: "192.168.1.143", sshUser: "jack", chip: "M4 Pro", ramGB: 24, usableGB: 15.5, backend: .metal,
             rpcServerPath: "~/halo/llama.cpp/build/bin/ggml-rpc-server", notes: "Daily-driver Mac mini; measured 40% below Odin when in use. Thunderbolt Ethernet en16."),
        Host(name: "Odin", wiredIP: "192.168.1.196", sshUser: "jack", chip: "M4 Pro", ramGB: 24, usableGB: 15.5, backend: .metal,
             rpcServerPath: "~/halo/llama.cpp/build/bin/ggml-rpc-server", notes: "macOS 26.6.2, native build. ~220 GB/s effective. 14B: 26.8 tok/s."),
        Host(name: "Overgaard", wiredIP: "192.168.1.61", sshUser: "jack", chip: "M4 Max", ramGB: 36, usableGB: 21.0, backend: .metal,
             rpcServerPath: "~/halo/bin/ggml-rpc-server", llamaBenchPath: "~/halo/bin/llama-bench", notes: "Best single machine (14B: 39.4 tok/s). Usable Metal well under advertised 30 GB; 24.8 GB share failed. Binaries in ~/halo/bin (llama-bench, ggml-rpc-server)."),
        Host(name: "Asgard", wiredIP: "192.168.1.184", sshUser: "jack", chip: "M2 Pro", ramGB: 16, usableGB: 5.0, backend: .metal,
             rpcServerPath: "~/halo/bin/ggml-rpc-server", notes: "Busy server (24 sessions, NFS). ~5 GB GPU to spare. en0 flapped once -- verify before use."),
        Host(name: "Heimdall", wiredIP: "192.168.1.50", sshUser: "jack", chip: "Ryzen 5900X + RTX 4070", ramGB: 62, usableGB: 9.0, backend: .cuda,
             rpcServerPath: "~/halo/llama.cpp/build-cuda/bin/ggml-rpc-server", notes: "Reliable CUDA worker at 6-9 GB. CPU-only inference hits 93 C in 2 min -- never a fallback path."),
        Host(name: "Jotunheim", wiredIP: "192.168.1.10", sshUser: "jack", chip: "M1 Pro", ramGB: 16, usableGB: 10.0, backend: .metal,
             rpcServerPath: "~/halo/bin/ggml-rpc-server", notes: "Half an Odin (0.5x decode). Plan on a 8-10 GB shard."),
        Host(name: "Vanaheim", wiredIP: "", sshUser: "jack", chip: "M1", ramGB: 8, usableGB: 5.0, backend: .metal,
             rpcServerPath: "~/halo/bin/ggml-rpc-server", notes: "Weak-node run only (R5)."),
        // Cloud standby servers (ADR-013 D76) as CPU shard-holders for real-WAN tests (Test 04a).
        // IP = Tailscale address once installed; SSH as root. Build ggml-rpc-server CPU-only there first.
        Host(name: "Chicago (cloud)", wiredIP: "", sshUser: "root", chip: "Linode 4GB, CPU", ramGB: 4, usableGB: 1.5, backend: .cpu,
             rpcServerPath: "~/halo/llama.cpp/build/bin/ggml-rpc-server", notes: "us-central, ~30 ms from the lab. Use the Tailscale IP. 1.5 GB share max."),
        Host(name: "Sydney (cloud)", wiredIP: "", sshUser: "root", chip: "Linode 4GB, CPU", ramGB: 4, usableGB: 1.5, backend: .cpu,
             rpcServerPath: "~/halo/llama.cpp/build/bin/ggml-rpc-server", notes: "ap-southeast, ~180 ms. The far-member floor. Use the Tailscale IP."),
        Host(name: "Volunteer (remote)", wiredIP: "", sshUser: "", chip: "", ramGB: 16, usableGB: 8, backend: .metal,
             rpcServerPath: "", managed: false, notes: "Fill in their Tailscale IP, chip, and usable memory. They start their own worker per docs/HALO-REMOTE-TESTER-SETUP.md."),
    ]
}

// MARK: - Test plan

/// What one run does. `workers` are in `--rpc` order; `--tensor-split` is emitted RPC-first,
/// local-last -- the inversion that cost most of Test 01's first night.
struct TestPlan: Codable, Hashable {
    var title: String = ""
    /// Which fleet machine runs llama-bench. Nil = this Mac. Any other host is driven over SSH
    /// (live session, so macOS Local Network rules don't bite), output streamed back here.
    var hostID: UUID? = nil
    var modelPath: String = ""
    var workers: [WorkerShare] = []
    var localShareGB: Double = 8
    var promptTokens: Int = 512
    var genTokens: Int = 128
    var repetitions: Int = 3
    var extraArgs: String = ""
    var restartWorkers: Bool = true
    var notes: String = ""

    struct WorkerShare: Codable, Hashable, Identifiable {
        var id: UUID = UUID()
        var hostID: UUID
        var shareGB: Double
    }

    var isSplit: Bool { !workers.isEmpty }

    /// `--tensor-split` in the order llama.cpp actually assigns: RPC devices first, local GPU last.
    var tensorSplit: String {
        (workers.map { fmt($0.shareGB) } + [fmt(localShareGB)]).joined(separator: "/")
    }
    private func fmt(_ g: Double) -> String {
        g == g.rounded() ? String(Int(g)) : String(format: "%.1f", g)
    }

    func host(in fleet: [Host]) -> Host? { hostID.flatMap { id in fleet.first { $0.id == id } } }

    func arguments(fleet: [Host]) -> [String] {
        var a = ["-m", modelPath, "-p", String(promptTokens), "-n", String(genTokens), "-r", String(repetitions)]
        if isSplit {
            let eps = workers.compactMap { w in fleet.first { $0.id == w.hostID }?.rpcEndpoint }
            a += ["--rpc", eps.joined(separator: ","), "--tensor-split", tensorSplit]
        }
        let extra = extraArgs.split(separator: " ").map(String.init).filter { !$0.isEmpty }
        return a + extra
    }
}

/// Presets from Test 01b's plan (HALO-TEST-01-LAN-SPLIT.md, "Test 01b -- plan").
struct Preset: Identifiable {
    let id = UUID()
    let name: String
    let why: String
    let build: ([Host]) -> TestPlan
}

enum Presets {
    static func host(_ n: String, _ fleet: [Host]) -> Host? { fleet.first { $0.name == n } }

    /// "Next test up": the first preset never attempted; failing that, the first without a pass.
    static func next(given reports: [TestReport]) -> Preset {
        func titled(_ p: Preset) -> String { p.build([]).title }
        if let fresh = all.first(where: { p in !reports.contains { $0.title == titled(p) } }) { return fresh }
        return all.first { p in !reports.contains { $0.outcome == .pass && $0.title == titled(p) } } ?? all[0]
    }

    static let all: [Preset] = [
        Preset(name: "R3b - 70B Q4_K_M three-way: Overgaard 21 / Odin 13 / Heimdall 9 (no Jotunheim)",
               why: "Third data point after two identical R3 passes (4.97 / 4.99). Drops the M1 Pro hop and moves its 6 GB onto the host, which puts Overgaard right at its measured ceiling with -lm none. Two questions in one run: what does the slowest hop cost, and where is the host's real no-mmap limit.") { f in
            var p = TestPlan(); p.title = "R3b - 70B Q4_K_M three-way no Jotunheim"; p.extraArgs = "-lm none"
            p.hostID = host("Overgaard", f)?.id
            p.modelPath = "~/halo/models/Llama-3.3-70B-Instruct-Q4_K_M.gguf"; p.localShareGB = 21
            let shares: [String: Double] = ["Odin": 13, "Heimdall": 9]
            p.workers = ["Odin", "Heimdall"].compactMap { host($0, f) }.map { .init(hostID: $0.id, shareGB: shares[$0.name] ?? 4) }
            p.notes = "Compare against R3 (13/9/6/15): decode 4.97-4.99, prefill 51.2. Passes faster -> the Jotunheim hop's cost is measured. Fails with a host-side Metal OOM -> Overgaard's real no-mmap ceiling is under 21 GB + overhead; retry at 19 with Odin 15."
            return p
        },
        Preset(name: "Q8_0 discriminator (14B Q8, Odin worker)",
               why: "Decides whether Q8 quant is off the table for Halo over RPC. Same architecture as the 14B Q4 that runs at every split; only the quant differs. Ten minutes.") { f in
            var p = TestPlan(); p.title = "Q8_0 discriminator - 14B Q8_0 Overgaard+Odin"
            p.modelPath = "~/halo/models/Qwen_Qwen3-14B-Q8_0.gguf"; p.localShareGB = 7.7
            if let o = host("Odin", f) { p.workers = [.init(hostID: o.id, shareGB: 8)] }
            p.notes = "Fails with res=-3 -> Q8_0-over-RPC bug, file upstream, Halo uses Q4/Q5/Q6. Passes -> it's the 32B; next rung 32B Q4_K_M."
            return p
        },
        Preset(name: "R2 - 32B Q4_K_M, Odin 10 / Heimdall 6 / local 4",
               why: "First real 'impossible on one Mac' run. 19.8 GB model; Q8_0 is cleared (passed 2026-09-10 09:32), so this isolates the 32B itself. Small local share keeps a daily-driver host out of the way.") { f in
            var p = TestPlan(); p.title = "R2 - 32B Q4_K_M three-way"; p.extraArgs = "-lm none"
            p.modelPath = "~/halo/models/Qwen_Qwen3-32B-Q4_K_M.gguf"; p.localShareGB = 4
            p.workers = [host("Odin", f), host("Heimdall", f)].compactMap { $0 }.map { .init(hostID: $0.id, shareGB: $0.name == "Odin" ? 10 : 6) }
            p.notes = "Passes -> 32B Q8 failure was Q8-at-32B specific; next is R3 (70B Q4). Fails with res=-3 -> something about the 32B architecture over RPC; try 32B with Odin-only split before 70B."
            return p
        },
        Preset(name: "D1 - 14B Q4 three-way with Heimdall CUDA (pipeline check)",
               why: "R2 (32B Q4, 3-way) failed host-side with res=-3 at 09:42, workers clean. Before blaming the 32B, prove the 3-way Metal+CUDA pipeline itself works on the model that runs everywhere. Three minutes.") { f in
            var p = TestPlan(); p.title = "D1 - 14B Q4_K_M three-way Odin+Heimdall"
            p.modelPath = "~/halo/models/Qwen_Qwen3-14B-Q4_K_M.gguf"; p.localShareGB = 2
            p.workers = [host("Odin", f), host("Heimdall", f)].compactMap { $0 }.map { .init(hostID: $0.id, shareGB: $0.name == "Odin" ? 4 : 3) }
            p.notes = "Passes -> CUDA worker + 3-way pipeline fine; the 32B is the problem. Fails -> the 3-way/CUDA path is broken and R2's failure is not about the 32B."
            return p
        },
        Preset(name: "D2 - 32B Q4 two-way, Odin 16 / local 4 (no CUDA)",
               why: "Same 32B, Metal only. Separates 'this model over RPC' from 'this model with a CUDA hop'.") { f in
            var p = TestPlan(); p.title = "D2 - 32B Q4_K_M two-way Odin only"; p.extraArgs = "-lm none"
            p.modelPath = "~/halo/models/Qwen_Qwen3-32B-Q4_K_M.gguf"; p.localShareGB = 4
            if let o = host("Odin", f) { p.workers = [.init(hostID: o.id, shareGB: 16)] }
            p.notes = "Odin's real budget is ~15.5-18 GB; 16 GB share is deliberately near the edge -- if it OOMs on Odin that shows in the worker log, distinct from a host-side res=-3."
            return p
        },
        Preset(name: "D3 - 32B Q4 solo on this Mac (needs ~21 GB: run on Overgaard)",
               why: "Does Qwen3-32B run on llama.cpp b10883 at all, with no RPC? If this fails too, it's the model on this tag -- file upstream / try a newer tag -- and nothing about Halo.") { f in
            var p = TestPlan(); p.title = "D3 - 32B Q4_K_M solo"
            p.hostID = host("Overgaard", f)?.id
            p.modelPath = "~/halo/models/Qwen_Qwen3-32B-Q4_K_M.gguf"
            p.notes = "Overgaard hosts (only lab Mac that holds 19.8 GB). Already passed 2026-09-10 10:08 over SSH: 164.0 / 18.2."
            return p
        },
        Preset(name: "Test 04a - real WAN: 14B Q4, Overgaard 8 / Chicago (cloud, CPU) 0.5",
               why: "First pooled run over the actual internet: Chicago holds a 0.5 GB CPU shard over Tailscale (relayed, 44 ms RTT). Prediction written 2026-09-10: Overgaard solo 25 ms/token + 1.1 x 22 ms one-way + ~40 ms CPU compute = ~90 ms/token, ~11 tok/s. Also times a real WAN shard upload. Needs Tailscale on Overgaard + Chicago and a CPU build of ggml-rpc-server on Chicago; put Chicago's Tailscale IP in Fleet first.") { f in
            var p = TestPlan(); p.title = "Test 04a - 14B Q4 Overgaard + Chicago CPU over Tailscale"; p.extraArgs = "-lm none"
            p.hostID = host("Overgaard", f)?.id
            p.modelPath = "~/halo/models/Qwen_Qwen3-14B-Q4_K_M.gguf"; p.localShareGB = 8
            if let c = host("Chicago (cloud)", f) { p.workers = [.init(hostID: c.id, shareGB: 0.5)] }
            p.notes = "Compare ms/token against Overgaard solo (39.4 tok/s = 25 ms) + 1.1 x one-way RTT to Chicago. Then repeat with Sydney (cloud) for the far-member floor."
            return p
        },
        Preset(name: "R3 - 70B Q4_K_M: Overgaard host 15 / Odin 13 / Heimdall 9 / Jotunheim 6",
               why: "The capability run: a 42.5 GB model no single lab machine can hold, across four nodes with every share inside its measured budget. Overgaard hosts (driven over SSH); Asgard left out -- flapping link, no free memory (2026-09-10 pre-flight). Expect 5-6 min of Load before anything else happens.") { f in
            var p = TestPlan(); p.title = "R3 - 70B Q4_K_M four-way"; p.extraArgs = "-lm none"
            p.hostID = host("Overgaard", f)?.id
            p.modelPath = "~/halo/models/Llama-3.3-70B-Instruct-Q4_K_M.gguf"; p.localShareGB = 15
            let shares: [String: Double] = ["Odin": 13, "Heimdall": 9, "Jotunheim": 6]
            p.workers = ["Odin", "Heimdall", "Jotunheim"].compactMap { host($0, f) }.map { .init(hostID: $0.id, shareGB: shares[$0.name] ?? 4) }
            p.notes = "Passes at any speed -> Halo's 'capability tier' framing holds on real hardware. Fails -> read the worker logs first: a Metal OOM on one node is a placement problem, a host-side res=-3 with clean workers is the 32B bug pattern on a second architecture."
            return p
        },
        Preset(name: "Solo baseline - 14B Q4 on this Mac, sustained",
               why: "Mini vs. MacBook Pro comparison (Midgaard idle vs. Odin), -n 1024.") { _ in
            var p = TestPlan(); p.title = "Solo 14B Q4_K_M sustained"
            p.modelPath = "~/halo/models/Qwen_Qwen3-14B-Q4_K_M.gguf"; p.genTokens = 1024
            return p
        },
    ]
}

// MARK: - Results

enum Outcome: String, Codable, CaseIterable {
    case pass, fail, partial, aborted
    var label: String {
        switch self { case .pass: "Pass"; case .fail: "Fail"; case .partial: "Partial"; case .aborted: "Aborted" }
    }
}

/// One filed report. Mirrors a markdown file in `docs/halo-reports/`; that folder is the source
/// of truth and History is rebuilt from it on every launch.
struct TestReport: Identifiable, Hashable {
    var id: String { fileURL.path }
    var fileURL: URL
    var title: String
    var date: Date
    var outcome: Outcome
    var model: String
    var placement: String
    var tensorSplit: String
    var pp512: Double?
    var tg128: Double?
    var summary: String
    var body: String   // full markdown
}

/// Parsed `llama-bench` result table row.
struct BenchRow: Hashable {
    var model: String
    var size: String
    var backend: String
    var test: String      // pp512, tg128, tg1024 ...
    var tokensPerSec: Double
    var stddev: Double?
}
