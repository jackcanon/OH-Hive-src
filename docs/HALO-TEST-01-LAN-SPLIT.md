# Halo Test 01 — LAN model split across Loki's Lab (llama.cpp RPC)

Written 2026-09-09 by Loki. First hands-on experiment for Project Halo (ADR-012 decision 17).
Substrate: llama.cpp's RPC backend, chosen because it is one binary, already the engine under
every node's Ollama, and the fastest path to a real number. This is a measurement, not a product
prototype — nothing here touches `ohhive-core`.

## Question this test answers

**Can a model that does not fit on any single lab machine actually run when its layers are split
across two to four of them over the LAN, and at what tokens/sec — compared with (a) the same model
CPU-only on the one box with enough system RAM, and (b) a model that fits on one node, to isolate
pure RPC overhead?**

Secondary, free because the setup is the same: what happens when a shard-holder dies
mid-generation, and does a Metal + CUDA + CPU pipeline produce the same tokens as a single backend
at temperature 0.

## Fleet (probed 2026-09-09, us-west LAN)

| Host | Chip / GPU | RAM | GPU-usable (default) | OS | Role in test |
|---|---|---|---|---|---|
| Midgaard | M4 Pro | 24 GB | ~18 GB | macOS | **host** (runs `llama-bench`/`llama-server`, holds first layers) |
| Odin | M4 Pro | 24 GB | ~18 GB | macOS | rpc worker |
| Asgard | M2 Pro | 16 GB | ~12 GB | macOS | rpc worker |
| Heimdall | Ryzen 5900X + RTX 4070 | 62 GB / 12 GB VRAM | 12 GB (CUDA) | Ubuntu | rpc worker (CUDA) **and** CPU-only baseline box |
| Vanaheim | M1 | 8 GB | ~6 GB | macOS | optional "weak node" run only |
| Jotunheim | unknown | — | — | — | excluded until an SSH key is installed |

macOS caps GPU wired memory at roughly 75% of RAM. For the 70B run this is tight
(18 + 18 + 12 = 48 GB for a 42 GB model plus KV cache); raise it on the two M4 Pros before R3 with
`sudo sysctl iogpu.wired_limit_mb=21000` (reverts on reboot) or add Heimdall as the fourth shard.

## Models

Downloaded 2026-09-09 to `/Volumes/10TB JBOD/halo/models/` on Midgaard (symlinked as
`~/halo/models`; the internal disk didn't have room for 86 GB). `SHA256SUMS` sits alongside.
Only the host needs the files — RPC workers receive tensors over the wire and the `-c` cache flag
keeps them locally for reruns. Heimdall needs its own copy of M and L for the CPU baselines
(`scp` from Midgaard, ~77 GB, do it over the wire).

| Tag | File | Size | Fits on one lab node? | Purpose |
|---|---|---|---|---|
| S | `Qwen3-14B-Q4_K_M.gguf` | ~9 GB | yes (any Mac, and 4070) | RPC overhead calibration |
| M | `Qwen3-32B-Q8_0.gguf` | ~35 GB | **no** (GPU); yes CPU-only on Heimdall | 2-node capability |
| L | `Llama-3.3-70B-Instruct-Q4_K_M.gguf` | ~42 GB | **no** (GPU); yes CPU-only on Heimdall | 3–4-node capability |

Sources: `bartowski/` or `unsloth/` repos on Hugging Face; use `huggingface-cli download`. Verify
sha256 after transfer; a corrupt GGUF fails in confusing ways over RPC.

## Setup — DONE 2026-09-09 (tag `b10883`)

Pinned llama.cpp tag **`b10883`** everywhere. What actually happened, for the record:

- No `cmake`, `brew`, or `pip` on Odin, Asgard, or Heimdall. Midgaard built once with static
  libs and the Metal shader library embedded (`-DGGML_METAL_EMBED_LIBRARY=ON
  -DBUILD_SHARED_LIBS=OFF`) and the binaries were copied to Odin/Asgard (`~/halo/bin/`).
- This tag's RPC backend has an Apple RDMA-over-Thunderbolt transport that links against
  `libibverbs`, which macOS doesn't ship — **`-DGGML_RPC_RDMA=OFF` is required on Mac**.
- The worker binary is **`ggml-rpc-server`** in this tag (not `rpc-server`).
- Heimdall had the NVIDIA driver (CUDA 13.3 runtime) but no toolkit. Jack approved
  `apt install cuda-toolkit-13-3` (installed 2026-09-09, `nvcc` 13.3). Two builds exist there:
  `build/` (CPU + RPC, for the `-ngl 0` baselines) and `build-cuda/` (CUDA + RPC, sm_89; verified
  it sees the RTX 4070 with 11,893 MiB). Both used a user-local cmake in `~/.local/bin`.
- Heimdall's root disk is only 98 GB (21 GB free after CUDA), so its model copies live on the
  1.8 TB NVMe at `/mnt/ollama-store/halo/models/`, symlinked from `~/halo/models`.

```bash
# Midgaard (built from source)
cmake -B build -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON -DGGML_RPC=ON -DGGML_RPC_RDMA=OFF \
  -DLLAMA_CURL=OFF -DBUILD_SHARED_LIBS=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
# Heimdall CPU baseline build
cmake -B build -DGGML_RPC=ON -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
# Heimdall CUDA build (PATH needs /usr/local/cuda/bin)
cmake -B build-cuda -DGGML_CUDA=ON -DGGML_RPC=ON -DCMAKE_CUDA_ARCHITECTURES=89 -DLLAMA_CURL=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
```

Binary locations: Midgaard `~/halo/llama.cpp/build/bin/`; Heimdall `~/halo/llama.cpp/build/bin/`
(CPU) and `~/halo/llama.cpp/build-cuda/bin/` (CUDA); Odin and Asgard `~/halo/bin/`.
`iperf3`: Midgaard `/opt/homebrew/bin/iperf3`, Heimdall `/usr/bin/iperf3`, Odin/Asgard
`DYLD_FALLBACK_LIBRARY_PATH=~/halo/ip/lib ~/halo/ip/iperf3` (copied with its dylibs).

### Link health findings (2026-09-09, before any run)

- **Heimdall was on a USB Ethernet dongle** (Realtek RTL8156, `enxf44dad04e7ac`), measuring
  654 Mbit/s with heavy retransmits. Moved 2026-09-09 evening to the onboard Intel I225-V
  (`enp7s0`), port on the Flex 2.5G switch forced to 1 Gbps (the I225-V flapped at 2.5G auto-neg).
  Needed a reboot to clear a NetworkManager/systemd-networkd double-management tangle (both are
  still active on that box — worth disabling one eventually). Now 939 Mbit/s line-rate.
  **New address is `192.168.1.50`** via DHCP; the old `.231` is gone. Set a fixed-IP reservation
  in UniFi for MAC `7c:10:c9:3e:1c:ed` so it stops moving. Netplan file
  `/etc/netplan/60-halo-onboard-nics.yaml` also pre-configures the two Broadcom BCM57810 SFP+
  ports (`enp8s0f0/f1`, DHCP, lower metric) for whenever a real 10G switch port exists.
- **The Flex 2.5G's SFP+ "port 10" is a combo port with RJ45 port 9** — one uplink lane, two
  connectors. Inserting an SFP takes port 9 down and kills the switch's uplink. It cannot serve
  Heimdall while port 9 is the feed. 10G to Heimdall needs a switch with a second SFP+ port.
- **Asgard's wired link dropped once** during a transfer ("No route to host" for ~a minute) and
  `en0` reported `status: inactive` afterward while still answering pings on `.184`. Verify
  which interface `.184` actually is and that it's stable before R3, or R3 will fail
  mid-load in a way that looks like an RPC bug.
- Midgaard can reach Heimdall directly (`jack@192.168.1.50`), so nothing needs to hop via
  Asgard any more. Jotunheim still has no key installed from any lab machine.

### Wired IPs — use these and only these

Every Mac has both a wired and a Wi-Fi address on 192.168.1.0/24. The Wi-Fi ones measured
120–150 ms average RTT with 240–670 ms spikes from Midgaard (worse than a transatlantic link);
the wired ones are all under 1 ms. Binding a worker to the wrong one silently wrecks the run.

| Host | Wired IP | Wi-Fi IP (do not use) |
|---|---|---|
| Midgaard | 192.168.1.143 (en16, Thunderbolt Ethernet) | 192.168.1.8 |
| Odin | 192.168.1.196 | 192.168.1.210 |
| Asgard | 192.168.1.184 | 192.168.1.251 |
| Heimdall | 192.168.1.50 | — |

Start a worker on each shard node (the README is explicit that the RPC server is
unauthenticated and must never face an open network — bind to the wired LAN IP only):

```bash
# Odin / Asgard
~/halo/bin/ggml-rpc-server -H <wired-ip> -p 50052 -c          # -c = local tensor cache
# Heimdall (CUDA worker for R4; use build/bin/ instead for a CPU-RPC worker)
~/halo/llama.cpp/build-cuda/bin/ggml-rpc-server -H 192.168.1.50 -p 50052 -c
```

Verified 2026-09-09: Odin's worker starts (Metal reports 19,069 MB max working set on the
M4 Pro, matching the ~18 GB estimate above) and Midgaard connects to it on the wired IP.

Heimdall CPU-only baseline needs no worker; run `llama-bench` there directly with `-ngl 0`.

## Step 0 — network truth, before any model work

Do this first and record it; every later number is meaningless without it.

```bash
# from Midgaard, to each worker
ping -c 20 <ip>                    # RTT: expect <1 ms wired, 2–10 ms Wi-Fi
iperf3 -c <ip> -t 10               # bandwidth: expect ~940 Mbit/s GbE, 300–800 Wi-Fi
```

Also record whether each node is on Ethernet or Wi-Fi. A single Wi-Fi node will dominate the
pipeline; if any are, note it and consider moving them to a cable for the run.

## Run matrix

All runs: `-p 512 -n 128` (512-token prompt, 128 generated), 3 repetitions, report median.
`llama-bench` prints `pp512` (prefill tok/s) and `tg128` (decode tok/s) separately — keep both;
prefill is bandwidth-bound, decode is latency-bound, and Halo cares about the second.

```bash
B=~/halo/llama.cpp/build/bin/llama-bench
M=~/halo/models
```

| Run | Model | Placement | Command (host = Midgaard unless noted) | What it isolates |
|---|---|---|---|---|
```bash
ODIN=192.168.1.196:50052; ASGARD=192.168.1.184:50052; HEIM=192.168.1.50:50052
S=$M/Qwen_Qwen3-14B-Q4_K_M.gguf; MM=$M/Qwen_Qwen3-32B-Q8_0.gguf; L=$M/Llama-3.3-70B-Instruct-Q4_K_M.gguf
```

| Run | Model | Placement | Command (host = Midgaard unless noted) | What it isolates |
|---|---|---|---|---|
| R0 | S | Midgaard alone | `$B -m $S -p 512 -n 128 -r 3` | single-node reference |
| R1 | S | Midgaard + Odin | `$B -m $S --rpc $ODIN -p 512 -n 128 -r 3` | **pure RPC overhead** on a model that didn't need splitting |
| R2 | M | Midgaard + Odin | `$B -m $MM --rpc $ODIN -p 512 -n 128 -r 3` | 2-node capability (first "impossible on one Mac" run) |
| R2b | M | Heimdall CPU-only | on Heimdall: `$B -m M.gguf -ngl 0 -t 12 -p 512 -n 128 -r 3` | offloading baseline for R2 |
| R3 | L | Midgaard + Odin + Asgard | `--rpc $ODIN,$ASGARD` (raise wired limit first) | 3-node capability |
| R4 | L | Midgaard + Odin + Asgard + Heimdall | `--rpc $ODIN,$ASGARD,$HEIM` | **heterogeneous** Metal + CUDA pipeline (CPU-RPC until the toolkit is installed) |
| R4b | L | Heimdall CPU-only | on Heimdall: `-ngl 0 -t 12` | offloading baseline for R3/R4 |
| R5 | L | R4 + Vanaheim | add Vanaheim's wired IP | does one weak node drag the whole pipeline (expected: yes) |

**R1-wifi (free extra):** re-run R1 with `--rpc 192.168.1.210:50052` (Odin's Wi-Fi address).
That's a real ~120 ms-RTT, high-jitter link with zero setup — a WAN stand-in before R6.

Layer placement defaults to memory-proportional; override with `--tensor-split` (e.g.
`--tensor-split 18,18,12` for R3) if the default OOMs or places badly. Record the split used.

### Optional, same setup, no new hardware

**R6 — latency curve.** Heimdall is Linux, so it can shape its own link. Re-run R4 with:
```bash
sudo tc qdisc add dev <iface> root netem delay 25ms     # then 50ms, 100ms
sudo tc qdisc del dev <iface> root                        # cleanup
```
Only Heimdall's hop gets the delay, but that's enough to see decode tok/s fall with RTT — the
Petals finding (100 ms ≈ 30–70% slower) reproduced on our own hardware. Note: netem on the
worker side delays both directions of that hop.

**R7 — shard death.** Start a long generation via `llama-cli` on the R3 config (`-n 2000`), then
`kill` Asgard's `rpc-server` mid-stream. Record exactly what the host does: hang, crash, error
message, partial output. Expected: the whole generation dies with no recovery. That expected
failure is the finding — it's the gap Halo's checkpoint layer (ADR-005) would have to fill.

**R8 — cross-backend determinism.** Run `llama-cli` with `--temp 0 --seed 1 -n 64` on the same
prompt in R0-style (all-Metal), R4 (Metal + CUDA), and R4b (CPU). Diff the outputs. Expect small
divergences from float ordering; if they're large, that's a real finding for the "heterogeneous
quantization / backend" question.

## What to record per run

- llama.cpp tag, model file + sha256, split used, GPU wired limit setting
- `pp512` and `tg128` tok/s (median of 3), plus min/max
- per-node: RSS/GPU memory at steady state (`sudo powermetrics` on Mac, `nvidia-smi` on Heimdall)
- Step 0 ping/iperf for every hop used
- anything odd: OOMs, retries, which node was the bottleneck (`rpc-server` logs)

Put raw output in `docs/halo-test-01-results/` (one file per run) and summarise in a table at
the bottom of this doc.

## Predictions (write down before running; grade after)

- R1 vs R0: decode drops 20–40% from RPC round-trips alone on a sub-millisecond LAN.
- R2: 32B Q8 across two M4 Pros lands at 4–8 tok/s decode; R2b (Heimdall CPU) at 1.5–3.
- R3/R4: 70B Q4 across 3–4 nodes lands at 2–5 tok/s; R4b (Heimdall CPU) at 1–2. Adding Heimdall's
  CUDA card (R4) helps memory headroom but doesn't raise decode speed much — it's another hop.
- R5: Vanaheim drags everything to Vanaheim's speed.
- R6: roughly linear decode slowdown with injected RTT; 100 ms hurts more than any bandwidth
  limit would.
- R7: no recovery of any kind.

If the pooled runs beat the CPU baseline by 2× or more, Halo's "capability tier" framing
(models that otherwise can't run at all, batch agent work, not chat) holds on real hardware.
If they don't, the cheapest Halo is "buy one member a lot of RAM."

## Results — session 1, 2026-09-09 evening (raw files in `halo-test-01-results/`)

| Run | Model | Placement | pp512 tok/s | tg128 tok/s | Outcome |
|---|---|---|---|---|---|
| Step 0 | — | Midgaard→Odin/Asgard/Heimdall wired | 935–939 Mbit/s, 0.3–0.7 ms RTT | — | clean; Wi-Fi paths 17–100 ms avg, spikes to 490 ms |
| R0 | 14B Q4 | Midgaard (M4 Pro 24 GB) alone | 143.9 | 16.4 | ran; **Midgaard was heavily loaded** (10.7 GB in memory compressor) |
| R0-Odin | 14B Q4 | Odin (M4 Pro 24 GB) alone | 240.3 | 26.8 | ran; the honest M4 Pro reference — same chip as Midgaard, idle |
| R0-Overgaard | 14B Q4 | Overgaard (M4 Max 36 GB) alone | 388.6 | 39.4 | ran; best single machine in the lab |
| R1 | 14B Q4 | Midgaard + Odin (RPC, ~4 GB on Odin) | 165.2 | 18.7 | ran; **split faster than Midgaard alone** on both prefill and decode |
| R2 | 32B Q8 | every combination tried (see below) | — | — | never completed |
| R3 | 70B Q4 | every combination tried (see below) | — | — | never completed |
| R2b/R4b | — | Heimdall CPU-only | — | — | abandoned: CPU hit 93 °C in 2 min at 8 threads; tripped once |

**Predictions graded.** R1 prediction (20–40% decode loss from RPC) was wrong: a sub-millisecond
hop costs less than halving each GPU's weight traffic saves; the split was +14% decode, +15%
prefill over the same host alone. Everything else is ungraded because it didn't run.

### The blocker as it looked at the time (superseded — see the shard-size ladder below: it was a `--tensor-split` device-order error)

Every R2/R3 attempt died the same way on whichever **Mac** was an RPC *worker*: the worker's
Metal logged `command buffer failed with status 5 / Insufficient Memory` at first compute, then
the server asserted (`Unsuccessful graph computations are not supported with RPC`). Observed on
Odin (18 GB Metal budget) at 21, 15, 13, 12, 10, 8, and finally **4 GB** shares; on Asgard at 8
and 5 GB; on Midgaard at 12 GB. Things that did **not** fix it, each tested in isolation:
correct `--tensor-split` syntax, `-b 128 -ub 128`, removing the `-c` tensor cache, a native
build on Odin's actual OS (26.6.2 vs. the 27.0 build), and `GGML_METAL_NO_RESIDENCY=1`.

What *does* work: a Mac as the **host** with a 24–28 GB local Metal share (Overgaard, many
times); Heimdall's RTX 4070 as a **CUDA RPC worker** holding 6–9 GB (never failed once); Odin
running the same 14B locally at full speed; and Odin as a worker with a ~4 GB share of a
14B (R1). So the bug is specific to *Metal-backed `ggml-rpc-server` + this model class*, not
the machines, not the network. It needs a minimal repro against llama.cpp `b10883` (and a newer
tag) before any more big-model runs — likely a bug report, not a config change.

### Operational findings (each cost real time; all now in the doc above or here)

1. `llama-bench --tensor-split` takes **slashes** (`20/12/10/8`); commas define multiple test
   variants. Comma-separated splits silently put every weight on device 0.
2. On macOS 27, a `nohup … &` process that outlives its SSH session **cannot open LAN
   connections** (Local Network privacy). Workers only listen, so they're fine; the host must run
   in a live SSH session.
3. `ggml-rpc-server` is single-client and never notices a dead client; a crashed host leaves the
   worker blocked on a half-open socket. Restart every worker before every launch.
4. Device order for `--tensor-split` is local GPU first, then RPC servers in `--rpc` order
   (`llama-cli --rpc … --list-devices` prints it).
5. llama-bench opens ~8 short connections per worker before the real session; probing the port
   with `nc` right before a launch can collide with that.
6. Macs with both Wi-Fi and Ethernet on one subnet sometimes route new connections over Wi-Fi,
   where the AP's client isolation drops TCP silently. Turn Wi-Fi off on wired nodes for tests.
7. Midgaard (daily-driver Mac) had 10.7 GB in the memory compressor; a "24 GB" machine in use
   is a ~10 GB machine. Odin, the identical chip idle, benchmarked 60% faster.
8. Asgard (busy server: 24 sessions, NFS, load 4–7) has effectively ~5 GB of GPU to spare.
9. Odin is on macOS 26.6.2; the other Macs are 27.0. Binaries built on 27 run there but with a
   version warning; Odin now has its own native build in `~/halo/llama.cpp/build/bin/`.
10. Heimdall's CPU can't sustain all-core inference (93 °C in 2 min, one thermal trip). As a
    GPU worker it stayed at 57 °C CPU / 46 °C GPU — fine.
11. The `-c` tensor cache made first loads *slower* (writes every tensor to disk) and reruns only
    slightly faster; on a 2.5G LAN it isn't worth it.

### What Halo learns from session 1

- LAN pooling has **no speed penalty and can be a speed gain** (R1). The network was never the
  bottleneck; memory ceilings and software were.
- "Idle member machine" is a fiction: real desktops have a fraction of their nominal memory free.
  Halo's capability report needs *measured free GPU memory*, not chip specs.
- A heterogeneous Metal-host + CUDA-worker pipeline connected and loaded cleanly; only the
  Metal-worker bug stopped it from computing.
- The one big-RAM box (Heimdall) can't be the fallback path — cooling, not RAM, is its limit.

### Odin solo profile — 2026-09-09 17:45–17:54 (MacBook Pro, M4 Pro, 24 GB, macOS 26.6.2, on AC)

Run entirely on Odin, no network, llama.cpp `b10883` built natively on the machine. Models are the
GGUF blobs already in Odin's Ollama store plus our 14B. Standard = `-p 512 -n 128 -r 3`;
sustained = `-p 512 -n 1024 -r 3`. Raw: `odin-solo-profile.log`, `odin-solo-telemetry.csv`,
`odin-solo-run.sh`.

| Model | Quant | File size | pp512 tok/s | tg128 tok/s | tg1024 (sustained) |
|---|---|---|---|---|---|
| llama3.1 8B | Q4_K_M | 4.6 GB | 452.8 | 48.3 | — |
| gemma4 12B (it-qat) | Q4_0 | 6.5 GB | 296.1 | 30.3 | — |
| qwen3 14B (Ollama blob) | Q4_K_M | 8.6 GB | 240.9 | 26.0 | **25.2** (−3%) |
| qwen3 14B (bartowski) | Q4_K_M | 8.4 GB | 240.2 | 26.7 | — |
| mistral-small 24B | Q4_K_M | 13.3 GB | 144.7 | 16.9 | **16.7** (−1%) |
| qwen3.6 (21.7 GB) | — | 21.7 GB | — | — | **loads, then fails at first compute (`res = -3`, Metal insufficient memory)** |
| gemma3 4B, qwen3.5 9B | — | — | — | — | failed to load: architectures unknown to tag `b10883` (not a hardware result) |

Observations:
- Decode speed tracks file size almost exactly (memory-bandwidth-bound): ~220 GB/s effective
  on every rung. That is the number to put in the capability record — it predicts any model's
  decode speed on this chip from its file size alone.
- **No thermal fade.** 1024-token generations held within 1–3% of the 128-token figure on both
  the 14B and 24B, the OS reported `CPU_Speed_Limit` = 100 throughout (51 samples, 10 s apart),
  battery temperature stayed at 30.7–30.8 °C. On AC, this MacBook Pro sustains inference
  without throttling for at least the 9-minute window.
- **The real memory ceiling is ~18 GB, and it fails late.** The 21.7 GB model allocated fine
  and died at first compute — the identical failure signature seen on every Mac RPC worker
  last night. So a Metal worker with a nominal 4–12 GB shard was somehow committing more than
  ~18 GB. That is the lead for the RPC repro: look at what `ggml-rpc-server` allocates beyond
  the weights (full-graph compute buffers? KV for all layers?) on the worker side.
- Reproducibility: the 14B benchmarked 26.8 / 26.0 / 26.7 tok/s across three separate runs
  tonight (two different GGUFs of the same model). Tight enough to compare machines on.
- `mediaanalysisd` (Photos background analysis) was at 245% CPU before the run and was killed;
  it did not return during the window. Worth checking before any benchmark on a Mac with Photos.
- Midgaard (identical chip) scored 143.9 / 16.4 on the same 14B earlier under heavy load — 40%
  below Odin. The Mini-vs-MacBook comparison needs Midgaard idle; scheduled for Test 01b.

### Jotunheim solo profile — 2026-09-09 21:02–21:10 (MacBook Pro, M1 Pro, 16 GB, macOS 26.6.2, on AC)

Same method and binaries as Odin (Odin's native macOS-26 build copied over; same OS). SSH access
fixed: Asgard's config lacked a `Host jotunheim` entry, so it wasn't using the lab key — added.
Wired IP `192.168.1.10` (0.4 ms); `.223` is Wi-Fi (68 ms avg). Raw: `jotunheim-solo-profile.log`,
`jotunheim-solo-telemetry.csv`.

| Model | Size | Jotunheim pp512 / tg128 | Odin pp512 / tg128 | Jotunheim ÷ Odin (decode) |
|---|---|---|---|---|
| llama3.1 8B Q4_K_M | 4.6 GB | 256.6 / 26.2 | 452.8 / 48.3 | 0.54 |
| gemma4 12B Q4_0 | 6.5 GB | 178.6 / 22.2 | 296.1 / 30.3 | 0.73 |
| qwen3 14B Q4_K_M | 8.4 GB | 134.9 / 13.7 | 240.2 / 26.7 | 0.51 |
| qwen3 14B sustained (tg1024) | 8.4 GB | **13.6** (−1%) | 25.2 (−3%) | 0.54 |
| qwen3.8 27B Q4 (16.8 GB) | 16.8 GB | **loads, dies at compute** (`res = -3`) — over its ~11 GB budget | n/a | — |

- **Jotunheim is half an Odin**, consistently: ~0.5× decode across the ladder, matching the
  M1 Pro's 200 GB/s memory bus vs the M4 Pro's ~273 GB/s plus the older GPU. No thermal fade
  over 1024 tokens (48 telemetry samples, zero throttle events, battery temp flat 30.5–30.6 °C).
- **Usable GPU memory is ~11 GB** (16 GB × ~0.7). The 8.4 GB 14B fits; the 16.8 GB 27B does not.
  As a Halo worker, plan on a **~9–10 GB shard**.
- Good news from the edge case: llama.cpp `b10883` **does** load the Qwen 3.8 architecture (the
  failure was memory, not "unknown model type"). Qwen 3.8 27B is usable as the fleet model on the
  Macs that can hold 17 GB — Overgaard, Odin, Midgaard, not Jotunheim or Asgard.
- Is it still a player? **Yes, as a worker, no, as a host.** In a pipeline each node's cost is
  its share ÷ its bandwidth; give Jotunheim ~8 GB and it adds ~40 ms per token — about the same
  as Odin carrying 14 GB. So it's worth roughly 8 GB of extra pool capacity at a proportional
  speed cost, which is exactly what Halo's scheduler should be reasoning about. For the 70B run
  it lets Midgaard drop out entirely: Odin 14 / Jotunheim 8 / Overgaard 20.5 = 42.5 GB.

### Shard-size ladder — 2026-09-09 20:03–20:38 (Overgaard host, Odin worker)

Purpose: find the size at which a Metal RPC worker fails. Method: 14B Q4 split with Odin's
nominal share stepped 2 → 4 → 6 → 8 GB, then the 32B Q8 at 10 and 12 GB; fresh worker per rung;
Odin's wired/free memory sampled every 5 s. Raw: `ladder-odin-worker.log`, `ladder-odin-worker-2.log`,
`ladder-odin-worker.sh`, `R2-32B-Q8-CORRECTED.log`.

| Rung (as written) | Result | Odin wired Δ | What Odin actually held |
|---|---|---|---|
| 14B, `--tensor-split 6.4/2` | pass, 17.7 tg | +6.4 GB | **6.4 GB** |
| 14B, `4.4/4` | pass, 20.6 tg | +4.5 GB | 4.4 GB |
| 14B, `2.4/6` | pass, 26.0 tg | +2.5 GB | 2.4 GB |
| 14B, `0.4/8` | pass, 31.4 tg | +0.4 GB | 0.4 GB (loaded in 8 s — nothing to send) |
| 32B Q8, `24.8/10` | **fail**, Odin Metal OOM | to 16.5 GB (ceiling) | 24.8 GB |
| 32B Q8, `22.8/12` | **fail**, Odin OOM, 11 GB compressor | to 16.4 GB | 22.8 GB |

**The "Metal RPC worker bug" was a device-order error, mine.** `llama-cli --list-devices` prints
the local GPU first, but **`--tensor-split` assigns RPC devices first and the local GPU last.**
Every split written last night gave the *big* share to the Mac worker and the small one to the
host — "Odin 26 / Asgard 4 / Heimdall 8" was really Odin 26 GB. Every worker OOM, every
"only works up to ~4 GB" observation, and the SIGPIPEs (workers dying mid-upload of a share
they could never hold) follow from this one inversion. The wired-memory deltas above prove it to
the tenth of a gigabyte. Correct form for a host + N workers: `--tensor-split <rpc1>/<rpc2>/…/<local>`.

Also learned: **Overgaard's usable Metal budget is well under its advertised 30.15 GB** — a 24.8
GB Q8 share plus compute failed (`res = -3`, same signature as the Odin 21.7 GB edge case),
21.8 GB with `-b 128` also failed. Treat M-series usable GPU memory as ~65% of RAM under
llama.cpp RPC, not 75%.

**Remaining open item — the 32B Q8 still does not run over RPC even with correct splits.** With
Odin 12 / Heimdall (CUDA) 8 / Overgaard 14.8 — every node far inside its budget — the host
returned `res = -3` at warmup and *no worker ever compiled a kernel*; the compute was rejected
host-side before dispatch. The 14B Q4_K_M runs at every split. Leading hypothesis: the Q8_0
quantization through the RPC backend (a `supports_op`/scheduler issue), not memory. Discriminating
test queued: **Qwen3-14B-Q8_0** (15.7 GB; same architecture as the model that works, only the
quant differs) — pulled to Heimdall's NVMe at `/mnt/ollama-store/halo/models/` for tomorrow. If it
fails: Q8_0-over-RPC is the bug (file upstream, use Q4/Q5/Q6 quants for Halo). If it passes: it's
something about the 32B, and the 32B Q4_K_M (19.8 GB, on Midgaard) is the next rung.

Side results from the ladder: the 14B split between Overgaard and Odin ran at 17.7–31.4 tok/s
depending on placement, and with 95% of the model on Odin it ran **faster (31.4) than Odin
alone (26.8)** — the host handling sampling/output while the worker does the layers is a real
win on a 2.5G LAN. Odin's wired memory ran at ~1.0–1.2× its actual shard: the worker overhead
is small; the ceiling is the machine's, not the protocol's.

### Test 01b — plan (revised 20:40)

1. **Q8_0 discriminator**: 14B Q8_0, Overgaard host, Odin worker, `--tensor-split 8/7.7`.
   Ten minutes. Decides whether Q8 is off the table for Halo.
2. **R2 for real**: 32B at whichever quant works, Odin 12 / Heimdall 8 / Overgaard 14–18. Then
   **R3** (70B Q4_K_M, 42.5 GB): Odin 14 / Heimdall 9 / Overgaard 20 = 43 — just fits; add
   Asgard 4 for margin. All splits RPC-first.
3. **Mini vs. MacBook Pro** (Jack's ask): Midgaard idle vs. Odin, same 14B, `-n 1024`.
4. Then R7 (shard death), R8 (cross-backend determinism), Test 02 (latency curve).

## Out of scope for this test

WAN, NAT traversal, trust, prompt privacy, Parallax. Those are Test 02+ once we know the LAN
floor. Nothing in this doc is wired into the Hive scheduler or `shard_plan`.
