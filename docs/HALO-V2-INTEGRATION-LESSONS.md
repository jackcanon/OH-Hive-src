# Project Halo — lessons from Test 01/01b, as integration requirements for Hive v2

Written 2026-09-10 by Loki after the first two days of hands-on pooled-inference testing
(`HALO-TEST-01-LAN-SPLIT.md`, `halo-reports/`, `HALO-R3-STORY-2026-09-10.md`). Each lesson below
is stated as what v2's sharded runner / scheduler / capability model must do, with the evidence
that earned it. ADR-012 decision 17 points here; the follow-up Halo ADR should absorb these.
Cmd Work tracks each "must" as a work item (labels `halo`, `v2-integration`).

## L1 — The host must load with mmap off. Not a flag; a rule.

**Must:** when a run has RPC workers, the host loads the model with load-mode none
(llama.cpp `-lm none` / `use_mmap = false`). The sharded runner sets this itself.

**Why:** with mmap on Apple Silicon the host maps the *entire* GGUF into a Metal buffer regardless
of `--tensor-split` (verbose log: `MTL0_Mapped model buffer size = 18840 MiB` for the whole 32B on
the host, plus the worker's 7.3 GB share). The host's working set capped the pool, and every model
larger than the host failed with a bare host-side `res = -3`. With `-lm none` the host holds only
its share (4.08 GB measured for a 4 GB share). This single behavior explained every failure since
the night before: 14B passed everywhere; 32B passed only on the 36 GB host; 32B Q8 and 70B failed
on every host. With it fixed, the 70B ran.

**Cost:** no-mmap means the host reads the whole file from disk instead of mapping it — the 70B's
Load stage was ~5 minutes. Acceptable for a batch tier; the runner should surface it as expected,
not as a hang. Cmd Work: `a2b70cc1` (high).

## L2 — Placement: smallest pool that fits, fastest first.

**Must:** the scheduler chooses the minimum set of nodes whose *measured* free GPU memory covers
the model plus per-node overhead, ordered fastest-first; it adds a weaker node only when the pool
cannot hold the model without it. Never "all available nodes."

**Why:** 70B four-way with Jotunheim (M1 Pro, 6 GB share): 4.99 tok/s decode. Same model
three-way without it, share moved to the host: 6.67 — +34%. The M1 Pro hop + share cost ~50 ms per
token; keeping it when it wasn't needed cost a quarter of the throughput. But on the night the host
couldn't take the extra share, Jotunheim was the difference between running and not running.

**Per-node cost model, validated:** cost ≈ share ÷ that node's memory bandwidth, plus one hop.
Predicted from Jotunheim's solo profile the night before: ~40 ms/token for an 8 GB share.
Measured in the pipeline: ~50 ms for 6 GB. Within 10 ms — good enough to schedule on.

## L3 — Capability records carry measured numbers, not spec sheets.

**Must:** `hive.node_capabilities` (ADR-012 D16's RAM/VRAM/bandwidth fields) is populated by
measurement on the node: (a) usable GPU memory *under no-mmap RPC load*, (b) effective memory
bandwidth from a short decode probe (tok/s × bytes-per-token on a known model), (c) whether the
machine is currently loaded (free vs nominal). Nominal RAM is a hint, never an input to placement.

**Why:** two identical M4 Pros differed by 40% because one was a daily driver with 10.7 GB in the
compressor. The "usable ≈ 65% of RAM" rule we carried for a day was an mmap artifact — with
`-lm none` Overgaard held 21 GB + KV + compute cleanly against a "~21 GB" estimate that was itself
too low. Decode speed tracks file size almost exactly (~220 GB/s effective on M4 Pro, ~half on
M1 Pro), so one bandwidth number per chip predicts any model's decode from its size.
`capability.rs` already has `upload_mbps`/`download_mbps` fields that are `None` today — measure
them too; they become first-class scheduling inputs the moment Halo leaves the LAN.

## L4 — Worker launch is a contract, not a convention.

**Must:** the node core starts its RPC worker with an explicit device (`-d MTL0` / `-d CUDA0`),
bound to the wired interface only, restarted fresh before every job, with its own log captured.
The host verifies the worker's advertised device and free memory before shipping a shard.

**Why:** without `-d`, `ggml-rpc-server` served its BLAS/CPU device and aborted at first compute
(`unsupported op RMS_NORM`) — the host saw only "remote server crashed." Workers are single-client
and never notice a dead host; a crashed host leaves the next run blocked on a half-open socket.
Wi-Fi addresses on the same subnet measured 17–150 ms with spikes to 490 ms and silently wreck a
run; Macs with both interfaces sometimes route new connections over Wi-Fi.

## L5 — Diagnostics: the runner logs what the library says, and reads the workers.

**Must:** the sharded runner runs the engine with library logging on (llama-bench's `-v`
equivalent; `llama_log_set` not nulled), records per-device model/KV/compute buffer sizes at load,
and on any failure pulls the tail of every worker's log into the job record automatically.

**Why:** a day and a half of "silent `res = -3`" was llama-bench muting llama's own error lines. The
buffer-size lines (`MTL0_Mapped model buffer size`, `RPC0 model buffer size`) were the root cause
in plain text the first time they were allowed to print. The BLAS-device crash was only visible on
the worker. HaloBench does both now; the node core must too.

## L6 — Shard loss is a hard abort of the host. The checkpoint layer sits above the engine. (R7, measured)

**Must:** (a) the node core runs the inference engine as a **child process**, never in-process;
(b) generated tokens are checkpointed **as they stream** (prompt + text-so-far), not at exit;
(c) on shard loss the runner re-plans placement without the dead node (L2), reloads, re-prefills
prompt + partial, and continues — budgeting one full reload (minutes at no-mmap) per loss;
(d) the coordinator keeps a lease/heartbeat on the *host* (ADR-005), because that's the failure
the workers can't see. Also: `hive.cards.shard_plan` does not currently exist as a column in the
live DB despite ADR-012 D16 — add it, but don't populate it for real jobs until (a)–(d) exist.

**Why (R7, 2026-09-10 13:26):** 70B three-way, generation streaming at 6.7 tok/s; Odin's worker
SIGKILLed 69 s in. Within **one second** the host aborted mid-word (`Remote RPC server crashed` →
`recv failed` → `ggml_abort` on the RPC dispatcher thread, SIGABRT, crash report). No retry, no
timeout, no error returned to the caller — the process is simply gone. llama.cpp RPC has **no
failover of any kind**. The partial output (352 words) survived only because the run had a pty.
The surviving worker (Heimdall) saw the connection close and freed its 9 GB shard instantly —
no reaper needed for worker memory. Worker death is visible to the host in < 1 s, so no worker
heartbeat is needed for that direction.

**Still open:** one no-mmap upload to Odin dropped mid-transfer (`send failed`, worker healthy)
and was never explained; long shard uploads over volunteer links need a retry story.

## L7 — Latency is priced per token, and shard distribution is the real WAN wall. (Test 02, measured)

**Must:** (a) the scheduler prices each node's RTT to the host exactly like bandwidth (L2):
cost ≈ share ÷ bandwidth + ~1.1 × one-way delay per token; (b) pooled jobs over WAN are routed
toward long-prompt / short-output shapes (prefill tolerates latency, decode does not);
(c) shard transfer is parallel/chunked (multi-stream TCP or QUIC), shards are cached on workers
across jobs, and popular models are pre-placed — single-stream TCP over 100 ms delivers ~11 MB/s,
so a 9 GB shard takes ~15 minutes before the first token.

**Why (Test 02, 2026-09-10 13:55–14:40):** 70B three-way (Overgaard 21 / Odin 13 / Heimdall 9),
delay injected on Heimdall's hop only. Decode 6.67 → 5.51 → 4.87 → 3.88 tok/s at 0/25/50/100 ms;
per token 150 → 181 → 205 → 258 ms = each token pays ~1.1× the injected one-way delay, i.e. one
trip through the slow leg per token — the D17 research model, reproduced on our hardware.
Prefill fell only 30% at 100 ms (batching amortizes the round trip). Step wall time 5:16 → 18:00,
almost all of it upload. Variance rose with latency (pp ±0.07 → ±1.89); jitter and loss are
Test 03.

**Real-WAN datapoint (Test 04a, 15:09):** first pooled run over the actual internet — Overgaard +
a Chicago Linode (2 vCPU, no GPU) holding a 0.5 GB shard over a Tailscale DERP relay, 44 ms RTT.
It worked: 6.66 tok/s decode, 20.6 prefill. But the CPU box's *compute* (not the latency) cost
~75–100 ms/token and cut prefill 95%, so: (d) **cloud CPU boxes are not shard-holders** at any
slice size, and (e) the capability record needs a per-node **prefill throughput** figure — compute
matters, not just bandwidth and RTT. The clean real-path latency number still needs a GPU worker
on the far end (Test 04b, a volunteer), or Heimdall's CUDA routed through the relay.

LAN curve, for reference: 14B two-way 18.7 → three-way 14.6; 32B two-way 11.9; 70B three-way 6.7
→ four-way 5.0. Prefill can *improve* with a fast worker (CUDA hop: 14B 165 → 251). Product
framing stands: pooled = "models that otherwise can't run at all; batch agent work, not chat" —
and over WAN, batch work with short outputs specifically.

## L8 — Same placement is deterministic; mixed backends are not. (R8, measured)

**Must:** (a) `shard_plan` records the full placement — nodes, backends, split, quant, seed — so a
result is reproducible on the same pool; (b) ADR-019 verification of pooled jobs is **semantic or
same-placement**, never byte-equality across different hardware; (c) one quant per pooled job
(mixed-quant shards untested; every field tool assumes one quant per instance).

**Why (R8, 2026-09-10 13:44):** 32B Q4, temp 0, seed 1, 64 tokens. Metal solo and Metal+Metal
over RPC: **byte-identical** — the RPC transport changes nothing. Metal+CUDA: identical for 42
words, then a near-tie flipped. CPU-only: diverged at word 24. All coherent; this is
floating-point summation order, not error. Heterogeneous Metal + CUDA + Metal pipelines computed
correctly at 14B, 32B and 70B; Q8_0 and Q4_K_M both work over RPC.
