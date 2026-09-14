import Foundation
import Network

/// Orchestrates one run: restart workers over SSH (they're single-client and never notice a dead
/// host -- Test 01 finding #3), wait for their ports, run `llama-bench` locally with live output,
/// parse the table. Everything observable for the UI goes through `BenchStore` on the main actor.
final class Runner {
    private var process: Process?
    private var remoteHost: Host?
    private(set) var isCancelled = false

    struct Result {
        var exitCode: Int32
        var rows: [BenchRow]
        var log: String
        var command: String
        var startedAt: Date
        var endedAt: Date
    }

    func cancel() {
        isCancelled = true
        // llama-bench runs under `script`; terminating the wrapper can orphan it, so kill both.
        let k = Process()
        if let r = remoteHost, !r.wiredIP.isEmpty {
            k.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
            k.arguments = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=6", "\(r.sshUser)@\(r.wiredIP)", "pkill -x llama-bench"]
        } else {
            k.executableURL = URL(fileURLWithPath: "/usr/bin/pkill"); k.arguments = ["-x", "llama-bench"]
        }
        try? k.run()
        process?.terminate()
    }

    /// Runs the whole plan. `emit` receives log lines as they arrive (already on the main actor).
    func run(plan: TestPlan, config: Config, emit: @escaping @MainActor (String) -> Void) async -> Result {
        let started = Date()
        var log = ""
        func say(_ s: String) async { log += s + "\n"; await emit(s) }

        let fleet = config.fleet
        let workers = plan.workers.compactMap { w in fleet.first { $0.id == w.hostID } }

        // 1. Workers.
        if plan.isSplit {
            for h in workers {
                guard !h.wiredIP.isEmpty else {
                    await say("!! \(h.name) has no wired IP configured -- fix it in Fleet.");
                    return Result(exitCode: -1, rows: [], log: log, command: "", startedAt: started, endedAt: Date())
                }
                if !h.managed {
                    await say("==> \(h.name) is unmanaged (no SSH): waiting for its owner's ggml-rpc-server at \(h.rpcEndpoint) -- up to 3 minutes")
                    if await Self.waitForPort(host: h.wiredIP, port: h.rpcPort, timeout: 180) {
                        await say("    \(h.name) is listening")
                    } else {
                        await say("!! \(h.name) never answered on \(h.rpcEndpoint). Ask its owner to start: ggml-rpc-server -H <their-tailscale-ip> -p \(h.rpcPort) -d \(h.effectiveDevice)")
                        return Result(exitCode: -2, rows: [], log: log, command: "", startedAt: started, endedAt: Date())
                    }
                    continue
                }
                if plan.restartWorkers {
                    await say("==> restarting ggml-rpc-server on \(h.name) (\(h.rpcEndpoint))")
                    let remote = "pkill -x ggml-rpc-server; sleep 1; nohup \(h.rpcServerPath) -H \(h.wiredIP) -p \(h.rpcPort) -d \(h.effectiveDevice) > ~/halo/rpc-server.log 2>&1 < /dev/null & disown; sleep 2; pgrep -x ggml-rpc-server >/dev/null && echo HALOBENCH_STARTED || echo HALOBENCH_FAILED"
                    await say("    device: -d \(h.effectiveDevice)")
                    let (out, code) = await Self.shell("/usr/bin/ssh", ["-o", "BatchMode=yes", "-o", "ConnectTimeout=6", "\(h.sshUser)@\(h.wiredIP)", remote])
                    await say(out.trimmingCharacters(in: .whitespacesAndNewlines))
                    if code != 0 || !out.contains("HALOBENCH_STARTED") {
                        await say("!! could not (re)start the worker on \(h.name) over SSH (exit \(code)). Check the lab key and the binary path.")
                    }
                }
                await say("    waiting for \(h.rpcEndpoint) ...")
                if await Self.waitForPort(host: h.wiredIP, port: h.rpcPort, timeout: 20) {
                    await say("    \(h.name) is listening")
                } else {
                    await say("!! \(h.name) never opened \(h.rpcEndpoint) -- aborting before llama-bench")
                    return Result(exitCode: -2, rows: [], log: log, command: "", startedAt: started, endedAt: Date())
                }
                if isCancelled { return Result(exitCode: -9, rows: [], log: log, command: "", startedAt: started, endedAt: Date()) }
            }
        }

        // 2. llama-bench -- on this Mac, or on a remote fleet host over SSH.
        let remote = plan.host(in: fleet)
        remoteHost = remote
        let proc = Process()
        let command: String
        if let r = remote {
            guard !r.wiredIP.isEmpty else {
                await say("!! \(r.name) has no wired IP configured -- fix it in Fleet.")
                return Result(exitCode: -1, rows: [], log: log, command: "", startedAt: started, endedAt: Date())
            }
            // Paths stay unexpanded (~) so the remote shell resolves them in the remote home.
            let bench = r.llamaBenchPath ?? config.llamaBenchPath
            let args = plan.arguments(fleet: fleet)
            command = ([bench] + args).joined(separator: " ")
            await say("==> [\(r.name)] \(command)")
            if plan.isSplit { await say("    tensor-split order: " + (workers.map(\.name) + [r.name]).joined(separator: " / ") + " (RPC first, host last)") }
            // Pre-flight the remote paths so a typo fails in a second, not after a 5-minute load.
            let (pre, _) = await Self.shell("/usr/bin/ssh", ["-o", "BatchMode=yes", "-o", "ConnectTimeout=6", "\(r.sshUser)@\(r.wiredIP)",
                "test -x \(bench) || echo NO_BENCH; test -f \(plan.modelPath) || echo NO_MODEL; pkill -x llama-bench; echo PREFLIGHT_OK"])
            if pre.contains("NO_BENCH") { await say("!! llama-bench not found on \(r.name) at \(bench) -- set it in Fleet."); return Result(exitCode: -3, rows: [], log: log, command: command, startedAt: started, endedAt: Date()) }
            if pre.contains("NO_MODEL") { await say("!! model not found on \(r.name): \(plan.modelPath)"); return Result(exitCode: -4, rows: [], log: log, command: command, startedAt: started, endedAt: Date()) }
            if !pre.contains("PREFLIGHT_OK") { await say("!! could not reach \(r.name) over SSH: \(pre.trimmingCharacters(in: .whitespacesAndNewlines))"); return Result(exitCode: -2, rows: [], log: log, command: command, startedAt: started, endedAt: Date()) }
            // Live SSH session (not nohup) so the remote host may open LAN connections; `script`
            // on the remote side gives llama-bench a pty so its rows stream instead of buffering.
            proc.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
            proc.arguments = ["-o", "BatchMode=yes", "-o", "ServerAliveInterval=30", "\(r.sshUser)@\(r.wiredIP)",
                              "script -q /dev/null " + command]
        } else {
            var p = plan
            p.modelPath = plan.modelPath.expandingTilde
            let args = p.arguments(fleet: fleet)
            let bench = config.llamaBenchURL
            command = ([bench.path] + args).joined(separator: " ")
            await say("==> \(command)")
            if plan.isSplit { await say("    tensor-split order: " + (workers.map(\.name) + ["local"]).joined(separator: " / ") + " (RPC first, local last)") }
            guard FileManager.default.isExecutableFile(atPath: bench.path) else {
                await say("!! llama-bench not found at \(bench.path) -- set the path in Settings.")
                return Result(exitCode: -3, rows: [], log: log, command: command, startedAt: started, endedAt: Date())
            }
            guard FileManager.default.fileExists(atPath: p.modelPath) else {
                await say("!! model not found: \(p.modelPath)")
                return Result(exitCode: -4, rows: [], log: log, command: command, startedAt: started, endedAt: Date())
            }
            // Run under a pty (`script -q /dev/null …`) so llama-bench line-buffers: when piped it
            // fully buffers stdout and the result rows only arrive at exit.
            proc.executableURL = URL(fileURLWithPath: "/usr/bin/script")
            proc.arguments = ["-q", "/dev/null", bench.path] + args
            proc.currentDirectoryURL = bench.deletingLastPathComponent()
        }
        let pipe = Pipe()
        proc.standardOutput = pipe
        proc.standardError = pipe
        self.process = proc

        let lines = LineBuffer()
        let stream = AsyncStream<String> { cont in
            pipe.fileHandleForReading.readabilityHandler = { fh in
                let d = fh.availableData
                if d.isEmpty { pipe.fileHandleForReading.readabilityHandler = nil; cont.finish(); return }
                for line in lines.push(d) { cont.yield(line) }
            }
            proc.terminationHandler = { _ in
                if let rest = lines.flush() { cont.yield(rest) }
                pipe.fileHandleForReading.readabilityHandler = nil
                cont.finish()
            }
        }

        do { try proc.run() } catch {
            await say("!! failed to launch llama-bench: \(error.localizedDescription)")
            return Result(exitCode: -5, rows: [], log: log, command: command, startedAt: started, endedAt: Date())
        }
        for await raw in stream {
            let line = raw.replacingOccurrences(of: "\r", with: "")
            if !line.isEmpty { await say(line) }
        }
        proc.waitUntilExit()
        let code = proc.terminationStatus
        await say("==> llama-bench exited \(code)")
        // On failure, the real error is usually on a worker -- pull each worker's log tail so the
        // report carries it (the 2026-09-10 BLAS-device crash was only visible on Odin).
        if code != 0, plan.isSplit, !isCancelled {
            for h in workers where !h.wiredIP.isEmpty && h.managed {
                await say("==> worker log tail: \(h.name) (~/halo/rpc-server.log)")
                let (out, _) = await Self.shell("/usr/bin/ssh", ["-o", "BatchMode=yes", "-o", "ConnectTimeout=6", "\(h.sshUser)@\(h.wiredIP)",
                    "pgrep -x ggml-rpc-server >/dev/null && echo '(worker still alive)' || echo '(worker is DEAD)'; grep -vE 'compile_pipeline|Accepted client|Client connection closed' ~/halo/rpc-server.log | tail -12"])
                for l in out.split(separator: "\n") { await say("    [\(h.name)] \(l)") }
            }
        }
        return Result(exitCode: code, rows: LlamaBenchParser.rows(in: log), log: log, command: command, startedAt: started, endedAt: Date())
    }

    // MARK: helpers

    static func shell(_ exe: String, _ args: [String]) async -> (String, Int32) {
        await withCheckedContinuation { cont in
            let p = Process(); p.executableURL = URL(fileURLWithPath: exe); p.arguments = args
            let pipe = Pipe(); p.standardOutput = pipe; p.standardError = pipe
            p.terminationHandler = { proc in
                let d = pipe.fileHandleForReading.readDataToEndOfFile()
                cont.resume(returning: (String(decoding: d, as: UTF8.self), proc.terminationStatus))
            }
            do { try p.run() } catch { cont.resume(returning: ("launch failed: \(error.localizedDescription)", -1)) }
        }
    }

    static func waitForPort(host: String, port: Int, timeout: TimeInterval) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if await probe(host: host, port: port) { return true }
            try? await Task.sleep(nanoseconds: 700_000_000)
        }
        return false
    }

    static func probe(host: String, port: Int) async -> Bool {
        await withCheckedContinuation { cont in
            guard let p = NWEndpoint.Port(rawValue: UInt16(port)) else { cont.resume(returning: false); return }
            let c = NWConnection(host: NWEndpoint.Host(host), port: p, using: .tcp)
            let once = Once()
            c.stateUpdateHandler = { st in
                switch st {
                case .ready: if once.claim() { c.cancel(); cont.resume(returning: true) }
                case .failed, .cancelled: if once.claim() { cont.resume(returning: false) }
                default: break
                }
            }
            c.start(queue: .global())
            DispatchQueue.global().asyncAfter(deadline: .now() + 1.5) {
                if once.claim() { c.cancel(); cont.resume(returning: false) }
            }
        }
    }
}

/// Splits a byte stream into lines across reads. Serialised by its own lock so it can be touched
/// from the pipe's readability handler and the termination handler.
final class LineBuffer: @unchecked Sendable {
    private var data = Data()
    private let lock = NSLock()
    func push(_ d: Data) -> [String] {
        lock.lock(); defer { lock.unlock() }
        data.append(d)
        var out: [String] = []
        while let nl = data.firstIndex(of: 10) {
            out.append(String(decoding: data[data.startIndex..<nl], as: UTF8.self))
            data.removeSubrange(data.startIndex...nl)
        }
        return out
    }
    func flush() -> String? {
        lock.lock(); defer { lock.unlock() }
        guard !data.isEmpty else { return nil }
        let s = String(decoding: data, as: UTF8.self); data.removeAll(); return s
    }
}

final class Once: @unchecked Sendable {
    private var used = false
    private let lock = NSLock()
    func claim() -> Bool { lock.lock(); defer { lock.unlock() }; if used { return false }; used = true; return true }
}
